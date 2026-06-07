# yt-dlp Modern GUI


yt-dlp を使用してビデオをダウンロードするための最新のクロスプラットフォーム対応デスクトップアプリケーションです。
Tauri 2.0（Rust）と SvelteKit で構築された、ビデオダウンロードを管理するための清潔で直感的なインターフェースを提供します。

[**English**](../README.md) | [**한국어**](README.ko.md) | **日本語** | [**中文(简体)**](README.zh-CN.md) | [**中文(繁體)**](README.zh-TW.md) | [**Español**](README.es.md) | [**Français**](README.fr.md) | [**Deutsch**](README.de.md) | [**Português**](README.pt-BR.md) | [**Русский**](README.ru.md) | [**Tiếng Việt**](README.vi.md)

## 機能

- フォーマットと品質を選択してビデオとプレイリストをダウンロード
- クロスプラットフォーム対応（Windows、macOS、Linux）
- キャンセルと再試行に対応した並行ダウンロードキュー
- 検索できるダウンロード履歴
- yt-dlp 向けのすっきりしたデスクトップ UI

<details>
<summary>高度な機能</summary>

- yt-dlp と FFmpeg の自動依存関係検出とインストールガイド
- ファイル名テンプレートのカスタマイズ（シンプルモードと詳細モード）
- 認証コンテンツ用のクッキーサポート
- 重複ダウンロード検出
- 多言語対応
- 4つのカラーテーマ（Dark、Violet、Red、Light）

</details>

> **💡 ヒント:** アプリは初回起動時に yt-dlp、FFmpeg、Deno を自動的にセットアップします（アプリに同梱、または必要に応じてダウンロード・更新）。自動管理される yt-dlp ビルドは実行のたびに自己展開するため、初回起動が遅くなる場合があります。**大幅に高速な**メタデータ取得とダウンロードのため、システムのパッケージマネージャーで事前にインストールすることをお勧めします — macOS は [Homebrew](https://brew.sh/)（`brew install yt-dlp ffmpeg`）、Windows は [winget](https://learn.microsoft.com/windows/package-manager/winget/)（`winget install yt-dlp.yt-dlp ffmpeg`）、Linux は `apt`/`pacman`。デフォルトでは、アプリはシステム PATH にインストールされたバージョンを検出して優先使用します。

## ソースからビルド

### 前提条件

- [Rust](https://www.rust-lang.org/tools/install)（最新の stable バージョン）
- [Node.js](https://nodejs.org/)（v18+）
- [Bun](https://bun.sh/)（パッケージマネージャー）
- [Tauri 2.0](https://v2.tauri.app/start/prerequisites/) のプラットフォーム別依存関係

### ビルド手順

```bash
# リポジトリをクローン
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# フロントエンドの依存関係をインストール
bun install

# 開発モードで実行
bun run tauri dev

# プロダクションビルド
bun run tauri build
```

プロダクションビルドの出力は `src-tauri/target/release/bundle/` に生成されます。

## ロードマップ

1. モバイルユーザー向けダウンローダーアプリ（yt-dlpサーバーを自分でホスティングできます）
2. バージョンアップデーター

## クレジット・サードパーティライセンス

このアプリは以下のオープンソースバイナリをバンドルまたはダウンロードします：

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg は GNU General Public License v3 のもとでライセンスされています。各リリースに同梱される GPL ビルドは上記リンクから確認でき、ソースコードは FFmpeg プロジェクトおよびビルド提供者から入手できます。

## ライセンス

このプロジェクトは [MIT License](../LICENSE) の下でライセンスされています。
