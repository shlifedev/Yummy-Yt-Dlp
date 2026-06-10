# yt-dlp Modern GUI


Một ứng dụng máy tính để bàn hiện đại, đa nền tảng để tải xuống video bằng yt-dlp.
Được xây dựng bằng Tauri 2.0 (Rust) và SvelteKit, cung cấp một giao diện sạch sẽ và trực quan để quản lý tải xuống video.

[**English**](../README.md) | [**한국어**](README.ko.md) | [**日本語**](README.ja.md) | [**中文(简体)**](README.zh-CN.md) | [**中文(繁體)**](README.zh-TW.md) | [**Español**](README.es.md) | [**Français**](README.fr.md) | [**Deutsch**](README.de.md) | [**Português**](README.pt-BR.md) | [**Русский**](README.ru.md) | **Tiếng Việt**

## Video

<p align="center">
  <img src="Video.gif" alt="Bản demo yt-dlp Modern GUI" width="700">
</p>

## Các tính năng

- Tải xuống video & danh sách phát với lựa chọn định dạng và chất lượng
- Hỗ trợ đa nền tảng (Windows, macOS, Linux)
- Hàng đợi tải xuống song song với tính năng hủy và thử lại
- Lịch sử tải xuống có thể tìm kiếm
- Giao diện desktop gọn gàng cho yt-dlp

<details>
<summary>Tính năng nâng cao</summary>

- Phát hiện tự động các phần phụ thuộc yt-dlp và FFmpeg với hướng dẫn cài đặt
- Tùy chỉnh mẫu tên tệp (chế độ đơn giản & nâng cao)
- Hỗ trợ cookie cho nội dung xác thực
- Phát hiện tải xuống trùng lặp
- Hỗ trợ đa ngôn ngữ
- 4 chủ đề màu sắc (Dark, Violet, Red, Light)

</details>

> **💡 Mẹo:** Ứng dụng tự động thiết lập yt-dlp, FFmpeg và Deno khi khởi chạy lần đầu (đi kèm với ứng dụng và được tải xuống/cập nhật khi cần). Bản dựng yt-dlp được quản lý tự động sẽ tự giải nén mỗi lần chạy, nên lần khởi động đầu tiên có thể chậm. Để truy xuất siêu dữ liệu và tải xuống **nhanh hơn đáng kể**, hãy cài đặt trước qua trình quản lý gói hệ thống — [Homebrew](https://brew.sh/) trên macOS (`brew install yt-dlp ffmpeg`), [winget](https://learn.microsoft.com/windows/package-manager/winget/) trên Windows (`winget install yt-dlp.yt-dlp ffmpeg`), hoặc `apt`/`pacman` trên Linux. Theo mặc định, ứng dụng tự động phát hiện và ưu tiên sử dụng phiên bản đã cài trong PATH hệ thống.

## Biên dịch từ mã nguồn

### Yêu cầu

- [Rust](https://www.rust-lang.org/tools/install) (phiên bản stable mới nhất)
- [Node.js](https://nodejs.org/) (v18+)
- [Bun](https://bun.sh/) (trình quản lý gói)
- Các phụ thuộc theo nền tảng cho [Tauri 2.0](https://v2.tauri.app/start/prerequisites/)

### Các bước

```bash
# Clone kho lưu trữ
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# Cài đặt các phụ thuộc frontend
bun install

# Chạy ở chế độ phát triển
bun run tauri dev

# Biên dịch cho môi trường sản xuất
bun run tauri build
```

Kết quả biên dịch sản xuất nằm trong `src-tauri/target/release/bundle/`.

## Ghi công & Giấy phép Bên thứ ba

Ứng dụng này đóng gói hoặc tải xuống các tệp nhị phân mã nguồn mở sau:

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg được cấp phép theo GNU General Public License v3. Bản build GPL chính xác đi kèm với mỗi phiên bản được liên kết ở trên, mã nguồn tương ứng có thể tìm thấy tại dự án FFmpeg và các nhà cung cấp bản build.

## Giấy phép

Dự án này được cấp phép theo [MIT License](../LICENSE).
