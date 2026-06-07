# yt-dlp Modern GUI

 
A modern, cross-platform desktop application for downloading videos using yt-dlp.
Built with Tauri 2.0 (Rust) and SvelteKit, providing a clean and intuitive interface for managing video downloads.

[**한국어**](docs/README.ko.md) | [**日本語**](docs/README.ja.md) | [**中文(简体)**](docs/README.zh-CN.md) | [**中文(繁體)**](docs/README.zh-TW.md) | [**Español**](docs/README.es.md) | [**Français**](docs/README.fr.md) | [**Deutsch**](docs/README.de.md) | [**Português**](docs/README.pt-BR.md) | [**Русский**](docs/README.ru.md) | [**Tiếng Việt**](docs/README.vi.md)

## Video

<p align="center">
  <img src="docs/Video.gif" alt="yt-dlp Modern GUI demo" width="700">
</p>

## Features

- Video & playlist download with format and quality selection
- Cross-platform support (Windows, macOS, Linux)
- Concurrent download queue with cancel and retry
- Download history with search
- Clean desktop UI built for yt-dlp

<details>
<summary>Advanced Features</summary>

- Automatic yt-dlp and FFmpeg dependency detection with installation guide
- Filename template customization (simple & advanced modes)
- Cookie support for authenticated content
- Duplicate download detection
- Multi-language support
- 4 color themes (Dark, Violet, Red, Light)

</details>

> **💡 Tip:** The app automatically sets up yt-dlp, FFmpeg, and Deno on first launch (bundled with the app and downloaded/updated as needed). The auto-managed yt-dlp build self-extracts on each run, so its first startup can be slow. For **significantly faster** metadata fetching and downloads, pre-install them via your system package manager — [Homebrew](https://brew.sh/) on macOS (`brew install yt-dlp ffmpeg`), [winget](https://learn.microsoft.com/windows/package-manager/winget/) on Windows (`winget install yt-dlp.yt-dlp ffmpeg`), or `apt`/`pacman` on Linux. By default the app detects and prefers the system-installed versions on your PATH.

## Build from Source

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Node.js](https://nodejs.org/) (v18+)
- [Bun](https://bun.sh/) (package manager)
- Platform-specific dependencies for [Tauri 2.0](https://v2.tauri.app/start/prerequisites/)

### Steps

```bash
# Clone the repository
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# Install frontend dependencies
bun install

# Run in development mode
bun run tauri dev

# Build for production
bun run tauri build
```

The production build output will be in `src-tauri/target/release/bundle/`.

## Release Deployment

Pushing to `main` runs the release workflow. It bumps the next patch version,
builds the signed Tauri artifacts, uploads the release files to GitHub Releases,
and publishes the updater payload to Cloudflare R2.

The updater is served from:

```text
https://patcher-server-yt-dlp.shlife.dev/yummy-yt-dlp/latest.json
```

GitHub Actions must have these repository settings:

- Secret: `CLOUDFLARE_API_TOKEN`
- Variable: `CLOUDFLARE_ACCOUNT_ID`
- Variable: `R2_BUCKET`
- Variable: `R2_PUBLIC_BASE_URL`
- Variable: `R2_RELEASE_PREFIX`

Current R2 layout:

- Bucket: `tauri-patch-server`
- Prefix: `yummy-yt-dlp`
- Versioned artifacts: `yummy-yt-dlp/releases/vX.Y.Z/<artifact>`
- Patch manifest: `yummy-yt-dlp/latest.json`

To patch users to a new version, merge or push the release commit to `main`.
The workflow overwrites `latest.json` with the new signed manifest after the
artifacts are uploaded to R2.

## Roadmap

1. Downloader app for mobile users (you can self-host your own yt-dlp server)
2. Version updater

## Credits & Third-party Licenses

This app bundles or downloads the following open-source binaries:

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg is licensed under the GNU General Public License v3. The exact GPL build shipped with each release is linked above, with corresponding source available from the FFmpeg project and the build providers.

## License

This project is licensed under the [MIT License](LICENSE).
