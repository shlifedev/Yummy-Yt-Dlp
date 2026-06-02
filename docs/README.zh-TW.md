# yt-dlp Modern GUI


一個現代化、跨平台的桌面應用程式，用於使用 yt-dlp 下載影片。
採用 Tauri 2.0（Rust）和 SvelteKit 構建，提供乾淨直觀的介面來管理影片下載。

[**English**](../README.md) | [**한국어**](README.ko.md) | [**日本語**](README.ja.md) | [**中文(简体)**](README.zh-CN.md) | [**中文(繁體)**](README.zh-TW.md) | [**Español**](README.es.md) | [**Français**](README.fr.md) | [**Deutsch**](README.de.md) | [**Português**](README.pt-BR.md) | [**Русский**](README.ru.md) | [**Tiếng Việt**](README.vi.md)

## 截圖

<p align="center">
  <img src="App.png" alt="yt-dlp Modern GUI" width="450">
</p>
<p align="center">
  <img src="Downloading.png" alt="yt-dlp Modern GUI" width="450">
</p>

## 功能

- 支援影片和播放清單下載，可選擇格式和畫質
- 並行下載佇列，支援取消和重試
- 下載歷史記錄，包含搜尋和管理功能
- 自動偵測 yt-dlp 和 FFmpeg 依賴項並提供安裝指南
- 檔案名稱樣板自訂（簡易和進階模式）
- Cookie 支援以下載需認證的內容
- 重複下載偵測
- 多語言支援（English、한국어、日本語、简体中文、繁體中文、Français、Deutsch）
- 4種顏色主題（Dark、Violet、Red、Light）
- 跨平台支援（Windows、macOS、Linux）

> **💡 提示：** 應用程式會在首次啟動時自動下載 yt-dlp、FFmpeg 和 Deno。但自動管理的二進位檔案（PyInstaller 打包）初次啟動可能較慢。為了**顯著提升**中繼資料擷取和下載速度，建議透過系統套件管理工具預先安裝 — macOS 使用 [Homebrew](https://brew.sh/)（`brew install yt-dlp ffmpeg`），Windows 使用 [winget](https://learn.microsoft.com/windows/package-manager/winget/)（`winget install yt-dlp.yt-dlp ffmpeg`），Linux 使用 `apt`/`pacman`。應用程式會自動偵測並優先使用系統 PATH 中已安裝的版本。

## 從原始碼建置

### 先決條件

- [Rust](https://www.rust-lang.org/tools/install)（最新 stable 版本）
- [Node.js](https://nodejs.org/)（v18+）
- [Bun](https://bun.sh/)（套件管理工具）
- [Tauri 2.0](https://v2.tauri.app/start/prerequisites/) 平台相關依賴

### 建置步驟

```bash
# 複製儲存庫
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# 安裝前端依賴
bun install

# 以開發模式執行
bun run tauri dev

# 正式環境建置
bun run tauri build
```

正式環境建置輸出位於 `src-tauri/target/release/bundle/`。

## 路線圖

1. 面向行動裝置使用者的下載器應用程式（可以自行託管 yt-dlp 伺服器）
2. 版本更新器

## 致謝與第三方授權

本應用程式捆綁或下載以下開源二進位檔：

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg 依據 GNU General Public License v3 授權。每個發行版所附帶的 GPL 建置版本可透過上方連結查看，相應原始碼可從 FFmpeg 專案及建置提供方取得。

## 授權

本專案採用 [MIT 授權](../LICENSE)。
