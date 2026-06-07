# yt-dlp Modern GUI


Una aplicación de escritorio moderna y multiplataforma para descargar videos usando yt-dlp.
Construida con Tauri 2.0 (Rust) y SvelteKit, proporcionando una interfaz limpia e intuitiva para administrar descargas de videos.

[**English**](../README.md) | [**한국어**](README.ko.md) | [**日本語**](README.ja.md) | [**中文(简体)**](README.zh-CN.md) | [**中文(繁體)**](README.zh-TW.md) | [**Français**](README.fr.md) | [**Deutsch**](README.de.md) | [**Português**](README.pt-BR.md) | [**Русский**](README.ru.md) | [**Tiếng Việt**](README.vi.md) | **Español**

## Video

<p align="center">
  <img src="Video.gif" alt="Demo de yt-dlp Modern GUI" width="700">
</p>

## Características

- Descarga de videos y listas de reproducción con selección de formato y calidad
- Soporte multiplataforma (Windows, macOS, Linux)
- Cola de descargas concurrentes con cancelación y reintentos
- Historial de descargas con búsqueda
- Interfaz de escritorio limpia para yt-dlp

<details>
<summary>Funciones avanzadas</summary>

- Detección automática de dependencias yt-dlp y FFmpeg con guía de instalación
- Personalización de plantillas de nombres de archivo (modo simple y avanzado)
- Soporte para cookies para contenido autenticado
- Detección de descargas duplicadas
- Soporte multiidioma
- 4 temas de colores (Dark, Violet, Red, Light)

</details>

> **💡 Consejo:** La aplicación configura automáticamente yt-dlp, FFmpeg y Deno en el primer inicio (incluidos con la aplicación y descargados/actualizados según sea necesario). La compilación de yt-dlp gestionada automáticamente se autoextrae en cada ejecución, por lo que su primer arranque puede ser lento. Para una obtención de metadatos y descargas **significativamente más rápidas**, instálelos previamente con su gestor de paquetes — [Homebrew](https://brew.sh/) en macOS (`brew install yt-dlp ffmpeg`), [winget](https://learn.microsoft.com/windows/package-manager/winget/) en Windows (`winget install yt-dlp.yt-dlp ffmpeg`), o `apt`/`pacman` en Linux. De forma predeterminada, la aplicación detecta y prioriza las versiones instaladas en el sistema desde su PATH.

## Compilar desde el código fuente

### Requisitos previos

- [Rust](https://www.rust-lang.org/tools/install) (última versión stable)
- [Node.js](https://nodejs.org/) (v18+)
- [Bun](https://bun.sh/) (gestor de paquetes)
- Dependencias específicas de plataforma para [Tauri 2.0](https://v2.tauri.app/start/prerequisites/)

### Pasos

```bash
# Clonar el repositorio
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# Instalar dependencias del frontend
bun install

# Ejecutar en modo desarrollo
bun run tauri dev

# Compilar para producción
bun run tauri build
```

La compilación de producción se genera en `src-tauri/target/release/bundle/`.

## Hoja de ruta

1. Aplicación de descarga para usuarios móviles (puede alojar su propio servidor yt-dlp)
2. Actualizador de versiones

## Créditos y Licencias de Terceros

Esta aplicación incluye o descarga los siguientes binarios de código abierto:

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg está licenciado bajo la GNU General Public License v3. La versión GPL exacta incluida en cada lanzamiento se enlaza arriba, con el código fuente correspondiente disponible en el proyecto FFmpeg y los proveedores de compilación.

## Licencia

Este proyecto está bajo la licencia [MIT License](../LICENSE).
