use crate::modules::logger;
use crate::ytdlp::security;
use crate::ytdlp::types::AdvancedOptions;

const CODECS: &[&str] = &["av01", "vp9", "h264"];
const SB_CATEGORIES: &[&str] = &[
    "sponsor",
    "intro",
    "outro",
    "selfpromo",
    "preview",
    "filler",
    "interaction",
    "music_offtopic",
];
const CONTAINER_FORMATS: &[&str] = &["mp4", "mkv", "webm"];
const SUB_CONVERT_FORMATS: &[&str] = &["srt", "ass", "vtt", "lrc"];

/// Build the extra yt-dlp CLI args contributed by the global advanced options.
///
/// Pure and deterministic so it can be unit-tested. Invalid or unset values are skipped silently
/// so a single bad option never blocks a download. Options that only make sense for video are
/// dropped when extracting audio, and ffmpeg-dependent flags are dropped when ffmpeg is missing
/// (mirroring how the rest of the executor degrades gracefully).
pub fn build_advanced_args(
    adv: &AdvancedOptions,
    is_audio: bool,
    ffmpeg_available: bool,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // --- Subtitles (video only) ---
    if !is_audio {
        let want_subs = adv.write_subs || adv.write_auto_subs || adv.embed_subs;
        if adv.write_subs {
            args.push("--write-subs".to_string());
        }
        if adv.write_auto_subs {
            args.push("--write-auto-subs".to_string());
        }
        if adv.embed_subs && ffmpeg_available {
            args.push("--embed-subs".to_string());
        }
        if want_subs {
            if let Ok(langs) = security::sanitize_sub_langs(&adv.sub_langs) {
                args.push("--sub-langs".to_string());
                args.push(langs);
            }
            if ffmpeg_available && SUB_CONVERT_FORMATS.contains(&adv.convert_subs.as_str()) {
                args.push("--convert-subs".to_string());
                args.push(adv.convert_subs.clone());
            }
        }
    }

    // --- SponsorBlock ---
    if adv.sponsorblock_mode == "mark" || adv.sponsorblock_mode == "remove" {
        let mut cats: Vec<String> = adv
            .sponsorblock_categories
            .iter()
            .filter(|c| SB_CATEGORIES.contains(&c.as_str()))
            .cloned()
            .collect();
        if cats.is_empty() {
            cats.push("sponsor".to_string());
        }
        // `remove` re-cuts the file and requires ffmpeg; without it, downgrade to non-destructive
        // `mark` (which at worst just won't embed the chapters) instead of letting yt-dlp error.
        let effective_remove = adv.sponsorblock_mode == "remove" && ffmpeg_available;
        if adv.sponsorblock_mode == "remove" && !ffmpeg_available {
            logger::warn_cat(
                "download",
                "SponsorBlock 'remove' requires ffmpeg; downgrading to 'mark'",
            );
        }
        args.push(if effective_remove {
            "--sponsorblock-remove".to_string()
        } else {
            "--sponsorblock-mark".to_string()
        });
        args.push(cats.join(","));
    }

    // --- Embedding & metadata ---
    if ffmpeg_available {
        if adv.embed_thumbnail {
            args.push("--embed-thumbnail".to_string());
            // 번들 ffmpeg엔 png 인코더가 없어 webp 썸네일을 mp4에 임베드할 때 yt-dlp의
            // png 변환이 "Encoder not found"로 죽는다. jpg(mjpeg)로 미리 변환해 우회.
            args.push("--convert-thumbnails".to_string());
            args.push("jpg".to_string());
        }
        if adv.embed_metadata {
            args.push("--embed-metadata".to_string());
        }
        if adv.embed_chapters {
            args.push("--embed-chapters".to_string());
        }
    }
    if adv.write_thumbnail {
        args.push("--write-thumbnail".to_string());
    }
    if adv.write_info_json {
        args.push("--write-info-json".to_string());
    }

    // --- Format / codec / speed ---
    if CODECS.contains(&adv.video_codec.as_str()) {
        args.push("--format-sort".to_string());
        args.push(format!("vcodec:{}", adv.video_codec));
    }
    if !adv.limit_rate.trim().is_empty() {
        if let Ok(rate) = security::sanitize_limit_rate(&adv.limit_rate) {
            args.push("--limit-rate".to_string());
            args.push(rate);
        }
    }

    // --- Network reliability ---
    let frags = adv.concurrent_fragments.clamp(1, 16);
    if frags > 1 {
        args.push("--concurrent-fragments".to_string());
        args.push(frags.to_string());
    }
    if let Some(retries) = adv.retries {
        args.push("--retries".to_string());
        args.push(retries.min(100).to_string());
    }
    if adv.sleep_interval > 0 {
        args.push("--sleep-interval".to_string());
        args.push(adv.sleep_interval.min(86_400).to_string());
    }

    // --- Container (video only) ---
    if !is_audio {
        if CONTAINER_FORMATS.contains(&adv.merge_output_format.as_str()) {
            args.push("--merge-output-format".to_string());
            args.push(adv.merge_output_format.clone());
        }
        if ffmpeg_available && CONTAINER_FORMATS.contains(&adv.remux_video.as_str()) {
            args.push("--remux-video".to_string());
            args.push(adv.remux_video.clone());
        }
    }

    // --- Sections / chapters (need ffmpeg) ---
    if ffmpeg_available {
        if !adv.download_sections.trim().is_empty() {
            if let Ok(sections) = security::sanitize_download_sections(&adv.download_sections) {
                let spec = if sections.starts_with('*') {
                    sections
                } else {
                    format!("*{}", sections)
                };
                args.push("--download-sections".to_string());
                args.push(spec);
            }
        }
        if adv.split_chapters {
            args.push("--split-chapters".to_string());
        }
    }

    // --- Proxy / timestamp / filename ---
    if !adv.proxy.trim().is_empty() {
        if let Ok(proxy) = security::sanitize_proxy(&adv.proxy) {
            args.push("--proxy".to_string());
            args.push(proxy);
        }
    }
    if adv.no_mtime {
        args.push("--no-mtime".to_string());
    }
    if adv.restrict_filenames {
        args.push("--restrict-filenames".to_string());
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(args: &[String], needle: &str) -> bool {
        args.iter().any(|a| a == needle)
    }
    fn val_after<'a>(args: &'a [String], flag: &str) -> Option<&'a String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    }

    #[test]
    fn subtitles_video() {
        let adv = AdvancedOptions {
            write_subs: true,
            embed_subs: true,
            sub_langs: "en,ko".to_string(),
            ..Default::default()
        };
        let args = build_advanced_args(&adv, false, true);
        assert!(has(&args, "--write-subs"));
        assert!(has(&args, "--embed-subs"));
        assert_eq!(val_after(&args, "--sub-langs"), Some(&"en,ko".to_string()));
    }

    #[test]
    fn subtitles_and_container_skipped_for_audio() {
        let adv = AdvancedOptions {
            write_subs: true,
            embed_subs: true,
            merge_output_format: "mkv".to_string(),
            remux_video: "mkv".to_string(),
            ..Default::default()
        };
        let args = build_advanced_args(&adv, true, true);
        assert!(!has(&args, "--write-subs"));
        assert!(!has(&args, "--embed-subs"));
        assert!(!has(&args, "--sub-langs"));
        assert!(!has(&args, "--merge-output-format"));
        assert!(!has(&args, "--remux-video"));
    }

    #[test]
    fn sponsorblock_mark_categories() {
        let adv = AdvancedOptions {
            sponsorblock_mode: "mark".to_string(),
            sponsorblock_categories: vec!["sponsor".to_string(), "intro".to_string()],
            ..Default::default()
        };
        let args = build_advanced_args(&adv, false, true);
        assert_eq!(
            val_after(&args, "--sponsorblock-mark"),
            Some(&"sponsor,intro".to_string())
        );
    }

    #[test]
    fn sponsorblock_remove_downgrades_without_ffmpeg() {
        let adv = AdvancedOptions {
            sponsorblock_mode: "remove".to_string(),
            ..Default::default()
        };
        let with = build_advanced_args(&adv, false, true);
        assert!(has(&with, "--sponsorblock-remove"));
        let without = build_advanced_args(&adv, false, false);
        assert!(has(&without, "--sponsorblock-mark"));
        assert!(!has(&without, "--sponsorblock-remove"));
    }

    #[test]
    fn embeds_need_ffmpeg_but_sidecars_dont() {
        let adv = AdvancedOptions {
            embed_thumbnail: true,
            embed_metadata: true,
            write_thumbnail: true,
            write_info_json: true,
            ..Default::default()
        };
        let no_ff = build_advanced_args(&adv, false, false);
        assert!(!has(&no_ff, "--embed-thumbnail"));
        assert!(!has(&no_ff, "--embed-metadata"));
        assert!(has(&no_ff, "--write-thumbnail"));
        assert!(has(&no_ff, "--write-info-json"));

        // With ffmpeg, embedding a thumbnail must pre-convert to jpg so the bundled
        // ffmpeg's missing png encoder can't break the mp4 thumbnail embed.
        let with_ff = build_advanced_args(&adv, false, true);
        assert!(has(&with_ff, "--embed-thumbnail"));
        assert_eq!(
            val_after(&with_ff, "--convert-thumbnails"),
            Some(&"jpg".to_string())
        );
    }

    #[test]
    fn concurrent_fragments_clamped() {
        let adv = AdvancedOptions {
            concurrent_fragments: 1,
            ..Default::default()
        };
        assert!(!has(
            &build_advanced_args(&adv, false, true),
            "--concurrent-fragments"
        ));
        let adv = AdvancedOptions {
            concurrent_fragments: 100,
            ..Default::default()
        };
        assert_eq!(
            val_after(
                &build_advanced_args(&adv, false, true),
                "--concurrent-fragments"
            ),
            Some(&"16".to_string())
        );
    }

    #[test]
    fn retries_option() {
        let adv = AdvancedOptions::default();
        assert!(!has(&build_advanced_args(&adv, false, true), "--retries"));
        let adv = AdvancedOptions {
            retries: Some(0),
            ..Default::default()
        };
        assert_eq!(
            val_after(&build_advanced_args(&adv, false, true), "--retries"),
            Some(&"0".to_string())
        );
        let adv = AdvancedOptions {
            retries: Some(999),
            ..Default::default()
        };
        assert_eq!(
            val_after(&build_advanced_args(&adv, false, true), "--retries"),
            Some(&"100".to_string())
        );
    }

    #[test]
    fn codec_format_sort() {
        let adv = AdvancedOptions::default(); // "auto"
        assert!(!has(
            &build_advanced_args(&adv, false, true),
            "--format-sort"
        ));
        let adv = AdvancedOptions {
            video_codec: "av01".to_string(),
            ..Default::default()
        };
        assert_eq!(
            val_after(&build_advanced_args(&adv, false, true), "--format-sort"),
            Some(&"vcodec:av01".to_string())
        );
    }

    #[test]
    fn download_sections_star_prefix_and_ffmpeg() {
        let adv = AdvancedOptions {
            download_sections: "1:30-2:00".to_string(),
            ..Default::default()
        };
        let args = build_advanced_args(&adv, false, true);
        assert_eq!(
            val_after(&args, "--download-sections"),
            Some(&"*1:30-2:00".to_string())
        );
        assert!(!has(
            &build_advanced_args(&adv, false, false),
            "--download-sections"
        ));
    }

    #[test]
    fn invalid_values_skipped() {
        let adv = AdvancedOptions {
            limit_rate: "1 MB/s".to_string(),
            proxy: "not a url".to_string(),
            ..Default::default()
        };
        let args = build_advanced_args(&adv, false, true);
        assert!(!has(&args, "--limit-rate"));
        assert!(!has(&args, "--proxy"));
    }

    #[test]
    fn proxy_and_misc() {
        let adv = AdvancedOptions {
            proxy: "http://127.0.0.1:8080".to_string(),
            no_mtime: true,
            restrict_filenames: true,
            ..Default::default()
        };
        let args = build_advanced_args(&adv, false, true);
        assert_eq!(
            val_after(&args, "--proxy"),
            Some(&"http://127.0.0.1:8080".to_string())
        );
        assert!(has(&args, "--no-mtime"));
        assert!(has(&args, "--restrict-filenames"));
    }
}
