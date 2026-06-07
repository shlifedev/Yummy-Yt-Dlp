# Third-Party Notices

This app uses sidecar command-line tools as separate programs. The app invokes
them through process execution and keeps user-selectable dependency paths in
Settings so users can replace app-managed copies with compatible system copies.

## yt-dlp

- Project: https://github.com/yt-dlp/yt-dlp
- Source license: The Unlicense
- Standalone release binaries downloaded by this app: GPLv3+
- Corresponding source: https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.tar.gz
- Third-party license details: https://github.com/yt-dlp/yt-dlp/blob/master/THIRD_PARTY_LICENSES.txt

The yt-dlp project documents that its source tarball and PyPI distributions
contain Unlicense code, while PyInstaller-bundled standalone executables include
GPLv3+ licensed code and are distributed as GPLv3+ combined works.

## FFmpeg

- Project source: https://ffmpeg.org
- Windows/Linux app-managed builds: https://github.com/BtbN/FFmpeg-Builds
- Windows/Linux build variant: `gpl`, licensed as GPLv3
- BtbN build scripts: https://github.com/BtbN/FFmpeg-Builds

The app no longer downloads the previous macOS FFmpeg build because its build
configuration enabled nonfree components. On macOS, users should use a system
FFmpeg installation, or app releases must ship a separately built redistributable
GPL/LGPL FFmpeg sidecar with matching source and build instructions.

## Deno

- Project: https://github.com/denoland/deno
- License: MIT
- Third-party license details: https://license.deno.dev/

## License Files

- GPLv3 text: `GPL-3.0.txt`
- Deno MIT license: `DENO-LICENSE.txt`
