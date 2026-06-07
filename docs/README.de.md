# yt-dlp Modern GUI


Eine moderne, plattformübergreifende Desktop-Anwendung zum Herunterladen von Videos mit yt-dlp.
Gebaut mit Tauri 2.0 (Rust) und SvelteKit, bietet eine saubere und intuitive Benutzeroberfläche zur Verwaltung von Video-Downloads.

[**English**](../README.md) | [**한국어**](README.ko.md) | [**日本語**](README.ja.md) | [**中文(简体)**](README.zh-CN.md) | [**中文(繁體)**](README.zh-TW.md) | [**Español**](README.es.md) | [**Français**](README.fr.md) | **Deutsch** | [**Português**](README.pt-BR.md) | [**Русский**](README.ru.md) | [**Tiếng Việt**](README.vi.md)

## Video

<p align="center">
  <img src="Video.gif" alt="yt-dlp Modern GUI Demo" width="700">
</p>

## Funktionen

- Video- und Playlist-Download mit Format- und Qualitätsauswahl
- Plattformübergreifende Unterstützung (Windows, macOS, Linux)
- Gleichzeitige Download-Warteschlange mit Abbruch und Wiederholung
- Durchsuchbarer Download-Verlauf
- Aufgeräumte Desktop-Oberfläche für yt-dlp

<details>
<summary>Erweiterte Funktionen</summary>

- Automatische Erkennung von yt-dlp- und FFmpeg-Abhängigkeiten mit Installationsanleitung
- Anpassung der Dateinamenvorlage (einfacher & erweiterter Modus)
- Cookie-Unterstützung für authentifizierte Inhalte
- Duplikat-Download-Erkennung
- Mehrsprachige Unterstützung
- 4 Farbthemen (Dark, Violet, Red, Light)

</details>

> **💡 Tipp:** Die App richtet yt-dlp, FFmpeg und Deno beim ersten Start automatisch ein (mit der App gebündelt und bei Bedarf heruntergeladen/aktualisiert). Der automatisch verwaltete yt-dlp-Build entpackt sich bei jedem Start selbst, daher kann der erste Start langsam sein. Für **deutlich schnellere** Metadaten-Abfragen und Downloads installieren Sie diese vorab über Ihren Paketmanager — [Homebrew](https://brew.sh/) auf macOS (`brew install yt-dlp ffmpeg`), [winget](https://learn.microsoft.com/windows/package-manager/winget/) auf Windows (`winget install yt-dlp.yt-dlp ffmpeg`), oder `apt`/`pacman` auf Linux. Standardmäßig erkennt und bevorzugt die App die im System-PATH installierten Versionen.

## Aus dem Quellcode bauen

### Voraussetzungen

- [Rust](https://www.rust-lang.org/tools/install) (neueste stable Version)
- [Node.js](https://nodejs.org/) (v18+)
- [Bun](https://bun.sh/) (Paketmanager)
- Plattformspezifische Abhängigkeiten für [Tauri 2.0](https://v2.tauri.app/start/prerequisites/)

### Schritte

```bash
# Repository klonen
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# Frontend-Abhängigkeiten installieren
bun install

# Im Entwicklungsmodus starten
bun run tauri dev

# Für Produktion bauen
bun run tauri build
```

Das Produktions-Build befindet sich in `src-tauri/target/release/bundle/`.

## Roadmap

1. Downloader-App für mobile Nutzer (Sie können Ihren eigenen yt-dlp-Server hosten)
2. Versions-Updater

## Danksagungen & Drittanbieter-Lizenzen

Diese App bündelt oder lädt die folgenden Open-Source-Binärdateien herunter:

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg ist unter der GNU General Public License v3 lizenziert. Der genaue GPL-Build, der mit jedem Release ausgeliefert wird, ist oben verlinkt; der zugehörige Quellcode ist beim FFmpeg-Projekt und den Build-Anbietern verfügbar.

## Lizenz

Dieses Projekt ist unter der [MIT-Lizenz](../LICENSE) lizenziert.
