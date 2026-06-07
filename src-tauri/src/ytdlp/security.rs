use crate::modules::types::AppError;
use std::net::IpAddr;
use std::path::Path;

/// Maximum allowed URL length (8KB - generous but prevents abuse)
const MAX_URL_LENGTH: usize = 8192;

/// Maximum allowed path length
const MAX_PATH_LENGTH: usize = 4096;

/// Maximum concurrent downloads allowed
const MAX_CONCURRENT_LIMIT: u32 = 10;

/// Allowed URL schemes
const ALLOWED_SCHEMES: &[&str] = &["http://", "https://"];

/// Known valid cookie browser names for yt-dlp's --cookies-from-browser
const VALID_COOKIE_BROWSERS: &[&str] = &[
    "brave", "chrome", "chromium", "edge", "firefox", "opera", "safari", "vivaldi", "whale",
];

/// Dangerous yt-dlp output template patterns that could cause path traversal or abuse
const DANGEROUS_TEMPLATE_PATTERNS: &[&str] = &[
    "..",   // path traversal
    "%(#)", // might expand unpredictably
];

/// Sanitize and validate a URL for safe use with yt-dlp.
///
/// This checks:
/// - URL is not empty and within length limits
/// - Only http/https schemes are allowed (blocks file://, data://, javascript://, etc.)
/// - SSRF protection: blocks localhost, loopback, private/link-local IP ranges
///
/// Note: This intentionally does NOT restrict to specific hostnames,
/// because yt-dlp supports 1000+ sites.
pub fn sanitize_url(url: &str) -> Result<String, AppError> {
    let url = url.trim();

    if url.is_empty() {
        return Err(AppError::InvalidUrl("URL is empty".to_string()));
    }

    if url.len() > MAX_URL_LENGTH {
        return Err(AppError::InvalidUrl(format!(
            "URL exceeds maximum length of {} characters",
            MAX_URL_LENGTH
        )));
    }

    // Check allowed schemes
    let lower = url.to_lowercase();
    if !ALLOWED_SCHEMES.iter().any(|s| lower.starts_with(s)) {
        return Err(AppError::InvalidUrl(
            "Only http:// and https:// URLs are supported".to_string(),
        ));
    }

    // Extract host for SSRF checks
    if let Some(host) = extract_host(&lower) {
        if is_ssrf_target(&host) {
            return Err(AppError::InvalidUrl(
                "URLs pointing to local or private network addresses are not allowed".to_string(),
            ));
        }
    }

    Ok(url.to_string())
}

/// Extract the hostname from a URL string (simple parser, no external deps).
fn extract_host(url: &str) -> Option<String> {
    // Skip scheme
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    // Strip userinfo (user:pass@)
    let after_userinfo = if let Some(at_pos) = after_scheme.find('@') {
        &after_scheme[at_pos + 1..]
    } else {
        after_scheme
    };

    // Handle IPv6 addresses in brackets: [::1], [fe80::1], etc.
    if after_userinfo.starts_with('[') {
        if let Some(bracket_end) = after_userinfo.find(']') {
            let host = &after_userinfo[..=bracket_end]; // includes brackets
            return if host.len() > 2 {
                Some(host.to_string())
            } else {
                None
            };
        }
        return None; // malformed IPv6
    }

    // Take until port, path, query, or fragment
    let host = after_userinfo
        .split([':', '/', '?', '#'])
        .next()
        .unwrap_or("");

    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

/// Check if a hostname is a potential SSRF target (local/private network).
fn is_ssrf_target(host: &str) -> bool {
    // Check common local hostnames
    if host == "localhost"
        || host == "localhost.localdomain"
        || host.ends_with(".localhost")
        || host == "[::]"
        || host == "[::1]"
    {
        return true;
    }

    // Try to parse as IP address
    // Strip brackets for IPv6
    let ip_str = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = ip_str.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => is_blocked_v4(v4),
            IpAddr::V6(v6) => {
                // Native v6 checks first so ::1 / :: are caught before to_ipv4()
                // (which would otherwise remap ::1 to 0.0.0.1 and slip through).
                if v6.is_loopback()        // ::1
                    || v6.is_unspecified() // ::
                    || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 (link-local)
                    || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fc00::/7 (unique local)
                {
                    return true;
                }
                // `::ffff:a.b.c.d` (IPv4-mapped) and the deprecated `::a.b.c.d`
                // (IPv4-compatible) forms both route to the embedded v4 address,
                // so re-check it as v4 to avoid bypassing the v4 allowlist.
                // to_ipv4() (not to_ipv4_mapped()) catches both forms.
                match v6.to_ipv4() {
                    Some(v4) => is_blocked_v4(v4),
                    None => false,
                }
            }
        };
    }

    false
}

/// Whether an IPv4 address falls in a loopback/private/link-local/reserved range
/// that should never be reachable for outbound downloads (SSRF protection).
fn is_blocked_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback()           // 127.0.0.0/8
        || v4.is_private()      // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
        || v4.is_link_local()   // 169.254.0.0/16
        || v4.is_unspecified()  // 0.0.0.0
        || v4.is_broadcast()    // 255.255.255.255
        || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64) // 100.64.0.0/10 (CGNAT)
}

/// Validate and sanitize a download output path.
///
/// Ensures the path:
/// - Is not empty and within length limits
/// - Is an absolute path
/// - Does not contain path traversal sequences after normalization
pub fn sanitize_output_path(path: &str) -> Result<String, AppError> {
    let path = path.trim();

    if path.is_empty() {
        return Err(AppError::FileError(
            "Download path cannot be empty".to_string(),
        ));
    }

    if path.len() > MAX_PATH_LENGTH {
        return Err(AppError::FileError(format!(
            "Path exceeds maximum length of {} characters",
            MAX_PATH_LENGTH
        )));
    }

    let p = Path::new(path);

    // Must be absolute
    if !p.is_absolute() {
        return Err(AppError::FileError(
            "Download path must be an absolute path".to_string(),
        ));
    }

    // Check for path traversal in any component
    for component in p.components() {
        if let std::path::Component::ParentDir = component {
            return Err(AppError::FileError(
                "Download path must not contain '..' traversal".to_string(),
            ));
        }
    }

    Ok(path.to_string())
}

/// Validate a yt-dlp filename template string.
///
/// Allows standard yt-dlp template variables like %(title)s, %(ext)s, etc.
/// Blocks path traversal and other dangerous patterns.
pub fn sanitize_filename_template(template: &str) -> Result<String, AppError> {
    let template = template.trim();

    if template.is_empty() {
        return Err(AppError::Custom(
            "Filename template cannot be empty".to_string(),
        ));
    }

    if template.len() > 500 {
        return Err(AppError::Custom(
            "Filename template is too long".to_string(),
        ));
    }

    for pattern in DANGEROUS_TEMPLATE_PATTERNS {
        if template.contains(pattern) {
            return Err(AppError::Custom(format!(
                "Filename template contains disallowed pattern: '{}'",
                pattern
            )));
        }
    }

    // Disallow absolute paths in template (should be relative, joined with output_dir)
    if Path::new(template).is_absolute() {
        return Err(AppError::Custom(
            "Filename template must be a relative path".to_string(),
        ));
    }

    Ok(template.to_string())
}

/// Validate the cookie browser name against known yt-dlp supported browsers.
pub fn sanitize_cookie_browser(browser: &str) -> Result<String, AppError> {
    let browser = browser.trim().to_lowercase();

    if browser.is_empty() {
        return Err(AppError::Custom(
            "Cookie browser name cannot be empty".to_string(),
        ));
    }

    // yt-dlp accepts browser names, optionally with profile: "chrome:Profile 1"
    // Extract just the browser name (before the colon)
    let browser_name = browser.split(':').next().unwrap_or(&browser);

    if !VALID_COOKIE_BROWSERS.contains(&browser_name) {
        return Err(AppError::Custom(format!(
            "Unsupported cookie browser: '{}'. Supported: {}",
            browser_name,
            VALID_COOKIE_BROWSERS.join(", ")
        )));
    }

    Ok(browser.to_string())
}

/// Clamp max_concurrent to a safe range [1, MAX_CONCURRENT_LIMIT].
pub fn clamp_max_concurrent(n: u32) -> u32 {
    n.clamp(1, MAX_CONCURRENT_LIMIT)
}

/// Sanitize error messages before sending to the frontend.
/// Removes potentially sensitive system paths.
pub fn sanitize_error_message(msg: &str) -> String {
    let mut sanitized = msg.to_string();

    // Replace common home directory patterns
    if let Ok(home) = std::env::var("HOME") {
        sanitized = sanitized.replace(&home, "~");
    }
    if let Ok(profile) = std::env::var("USERPROFILE") {
        sanitized = sanitized.replace(&profile, "~");
    }

    sanitized
}

/// Validate a yt-dlp subtitle language list (e.g. "en,ko,en-US", "all", "all,-live_chat").
/// Rejects whitespace and shell metacharacters that have no business in a lang list.
pub fn sanitize_sub_langs(langs: &str) -> Result<String, AppError> {
    let langs = langs.trim();
    if langs.is_empty() || langs.len() > 200 {
        return Err(AppError::Custom(
            "Subtitle languages must be 1-200 characters".to_string(),
        ));
    }
    let re = regex::Regex::new(r"^[A-Za-z0-9,.\-_*]+$").unwrap();
    if !re.is_match(langs) {
        return Err(AppError::Custom(
            "Subtitle languages may only contain letters, digits and , . - _ * (e.g. \"en,ko\")"
                .to_string(),
        ));
    }
    Ok(langs.to_string())
}

/// Validate a yt-dlp rate limit (e.g. "1M", "500K", "4.2M", "1000").
pub fn sanitize_limit_rate(rate: &str) -> Result<String, AppError> {
    let rate = rate.trim();
    let re = regex::Regex::new(r"^\d+(\.\d+)?[KMGkmg]?$").unwrap();
    if !re.is_match(rate) {
        return Err(AppError::Custom(
            "Rate limit must look like 1M, 500K or 4.2M (no spaces or units like MB/s)".to_string(),
        ));
    }
    let numeric = rate
        .trim_end_matches(|c: char| matches!(c, 'K' | 'M' | 'G' | 'k' | 'm' | 'g'));
    if numeric.parse::<f64>().ok().filter(|v| *v > 0.0).is_none() {
        return Err(AppError::Custom(
            "Rate limit must be greater than zero".to_string(),
        ));
    }
    Ok(rate.to_string())
}

/// Validate a yt-dlp download-sections time range (e.g. "1:30-2:00", "00:01:30-00:02:00",
/// optionally prefixed with "*"). Only a single time range is accepted.
pub fn sanitize_download_sections(sections: &str) -> Result<String, AppError> {
    let sections = sections.trim();
    let re =
        regex::Regex::new(r"^\*?(?:\d{1,2}:)?[0-5]?\d:[0-5]\d-(?:\d{1,2}:)?[0-5]?\d:[0-5]\d$")
            .unwrap();
    if !re.is_match(sections) {
        return Err(AppError::Custom(
            "Section must be a time range like 1:30-2:45 or 00:01:30-00:02:45".to_string(),
        ));
    }
    Ok(sections.to_string())
}

/// Validate a proxy URL for yt-dlp's --proxy. Unlike `sanitize_url`, this intentionally ALLOWS
/// localhost / private addresses, since proxies are very commonly local (e.g. 127.0.0.1:8080).
pub fn sanitize_proxy(proxy: &str) -> Result<String, AppError> {
    let proxy = proxy.trim();
    if proxy.is_empty() || proxy.len() > 2048 {
        return Err(AppError::Custom(
            "Proxy URL must be 1-2048 characters".to_string(),
        ));
    }
    let re =
        regex::Regex::new(r"^(?i)(https?|socks4|socks5)://[A-Za-z0-9.\-_]+(:\d{1,5})?/?$").unwrap();
    if !re.is_match(proxy) {
        return Err(AppError::Custom(
            "Proxy must be like http://host:port or socks5://127.0.0.1:1080".to_string(),
        ));
    }
    let without_slash = proxy.trim_end_matches('/');
    if let Some((_, port)) = without_slash.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit())
            && port.parse::<u16>().ok().filter(|p| *p > 0).is_none()
        {
            return Err(AppError::Custom(
                "Proxy port must be between 1 and 65535".to_string(),
            ));
        }
    }
    Ok(proxy.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // === URL sanitization tests ===

    #[test]
    fn test_valid_http_urls() {
        assert!(sanitize_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ").is_ok());
        assert!(sanitize_url("http://example.com/video").is_ok());
        assert!(sanitize_url("https://vimeo.com/123456").is_ok());
        assert!(sanitize_url("https://www.bilibili.com/video/BV1xx411c7XW").is_ok());
    }

    #[test]
    fn test_empty_url() {
        assert!(sanitize_url("").is_err());
        assert!(sanitize_url("   ").is_err());
    }

    #[test]
    fn test_disallowed_schemes() {
        assert!(sanitize_url("file:///etc/passwd").is_err());
        assert!(sanitize_url("javascript:alert(1)").is_err());
        assert!(sanitize_url("data:text/html,<h1>Hi</h1>").is_err());
        assert!(sanitize_url("ftp://example.com/file").is_err());
    }

    #[test]
    fn test_ssrf_localhost() {
        assert!(sanitize_url("http://localhost/admin").is_err());
        assert!(sanitize_url("http://localhost:8080/api").is_err());
        assert!(sanitize_url("http://127.0.0.1/secret").is_err());
        assert!(sanitize_url("http://127.0.0.2/secret").is_err());
    }

    #[test]
    fn test_ssrf_private_ips() {
        assert!(sanitize_url("http://10.0.0.1/internal").is_err());
        assert!(sanitize_url("http://172.16.0.1/internal").is_err());
        assert!(sanitize_url("http://192.168.1.1/internal").is_err());
        assert!(sanitize_url("http://169.254.169.254/metadata").is_err()); // AWS metadata
    }

    #[test]
    fn test_ssrf_ipv6() {
        assert!(sanitize_url("http://[::1]/secret").is_err());
        assert!(sanitize_url("http://[::]/secret").is_err());
    }

    #[test]
    fn test_ssrf_ipv4_mapped_ipv6() {
        // IPv4-mapped (::ffff:a.b.c.d) and IPv4-compatible (::a.b.c.d) forms must
        // not bypass the v4 allowlist by re-encoding a private/loopback address.
        assert!(sanitize_url("http://[::ffff:127.0.0.1]/secret").is_err());
        assert!(sanitize_url("http://[::ffff:169.254.169.254]/metadata").is_err());
        assert!(sanitize_url("http://[::ffff:10.0.0.1]/internal").is_err());
        assert!(sanitize_url("http://[::127.0.0.1]/secret").is_err());
    }

    #[test]
    fn test_ssrf_ipv4_mapped_public_allowed() {
        // A mapped *public* address must still be allowed (no false positives).
        assert!(sanitize_url("http://[::ffff:8.8.8.8]/").is_ok());
    }

    #[test]
    fn test_url_too_long() {
        let long_url = format!("https://example.com/{}", "a".repeat(MAX_URL_LENGTH));
        assert!(sanitize_url(&long_url).is_err());
    }

    // === Path sanitization tests ===

    #[test]
    fn test_valid_paths() {
        assert!(sanitize_output_path("/Users/test/Downloads").is_ok());
        if cfg!(target_os = "windows") {
            // Windows absolute paths - tested on Windows only
        }
    }

    #[test]
    fn test_path_traversal() {
        assert!(sanitize_output_path("/Users/test/../etc").is_err());
        assert!(sanitize_output_path("/tmp/../../etc/passwd").is_err());
    }

    #[test]
    fn test_relative_path_rejected() {
        assert!(sanitize_output_path("relative/path").is_err());
        assert!(sanitize_output_path("./local").is_err());
    }

    // === Filename template tests ===

    #[test]
    fn test_valid_templates() {
        assert!(sanitize_filename_template("%(title)s.%(ext)s").is_ok());
        assert!(sanitize_filename_template("%(uploader)s/%(title)s.%(ext)s").is_ok());
        assert!(sanitize_filename_template("%(upload_date)s-%(title)s-%(id)s.%(ext)s").is_ok());
    }

    #[test]
    fn test_template_path_traversal() {
        assert!(sanitize_filename_template("../../%(title)s.%(ext)s").is_err());
        assert!(sanitize_filename_template("../secret/%(title)s").is_err());
    }

    // === Cookie browser tests ===

    #[test]
    fn test_valid_browsers() {
        assert!(sanitize_cookie_browser("chrome").is_ok());
        assert!(sanitize_cookie_browser("firefox").is_ok());
        assert!(sanitize_cookie_browser("edge").is_ok());
        assert!(sanitize_cookie_browser("chrome:Profile 1").is_ok());
    }

    #[test]
    fn test_invalid_browsers() {
        assert!(sanitize_cookie_browser("malicious_browser").is_err());
        assert!(sanitize_cookie_browser("").is_err());
        assert!(sanitize_cookie_browser("/bin/sh").is_err());
    }

    // === Max concurrent tests ===

    #[test]
    fn test_clamp_max_concurrent() {
        assert_eq!(clamp_max_concurrent(0), 1);
        assert_eq!(clamp_max_concurrent(5), 5);
        assert_eq!(clamp_max_concurrent(100), MAX_CONCURRENT_LIMIT);
        assert_eq!(clamp_max_concurrent(u32::MAX), MAX_CONCURRENT_LIMIT);
    }

    // === Advanced option validators ===

    #[test]
    fn test_sub_langs() {
        assert!(sanitize_sub_langs("en").is_ok());
        assert!(sanitize_sub_langs("en,ko,en-US").is_ok());
        assert!(sanitize_sub_langs("all,-live_chat").is_ok());
        assert!(sanitize_sub_langs("en;rm -rf").is_err());
        assert!(sanitize_sub_langs("en | cat").is_err());
        assert!(sanitize_sub_langs("").is_err());
    }

    #[test]
    fn test_limit_rate() {
        assert!(sanitize_limit_rate("1M").is_ok());
        assert!(sanitize_limit_rate("500K").is_ok());
        assert!(sanitize_limit_rate("4.2M").is_ok());
        assert!(sanitize_limit_rate("1000").is_ok());
        assert!(sanitize_limit_rate("0").is_err());
        assert!(sanitize_limit_rate("0K").is_err());
        assert!(sanitize_limit_rate("0.0M").is_err());
        assert!(sanitize_limit_rate("1 MB/s").is_err());
        assert!(sanitize_limit_rate("fast").is_err());
    }

    #[test]
    fn test_download_sections() {
        assert!(sanitize_download_sections("1:30-2:45").is_ok());
        assert!(sanitize_download_sections("00:01:30-00:02:45").is_ok());
        assert!(sanitize_download_sections("*0:10-0:20").is_ok());
        assert!(sanitize_download_sections("30-90").is_err());
        assert!(sanitize_download_sections("1:30,2:45").is_err());
        assert!(sanitize_download_sections("1:99-2:00").is_err());
        assert!(sanitize_download_sections("1:30-2:99").is_err());
    }

    #[test]
    fn test_proxy() {
        assert!(sanitize_proxy("http://127.0.0.1:8080").is_ok());
        assert!(sanitize_proxy("https://proxy.example.com:3128").is_ok());
        assert!(sanitize_proxy("socks5://127.0.0.1:1080").is_ok());
        assert!(sanitize_proxy("http://proxy.example.com:65535").is_ok());
        assert!(sanitize_proxy("http://proxy.example.com:0").is_err());
        assert!(sanitize_proxy("http://proxy.example.com:65536").is_err());
        assert!(sanitize_proxy("http://proxy.example.com:99999").is_err());
        assert!(sanitize_proxy("javascript:alert(1)").is_err());
        assert!(sanitize_proxy("not a url").is_err());
    }
}
