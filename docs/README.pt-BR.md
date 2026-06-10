# yt-dlp Modern GUI


Um aplicativo de desktop moderno e multiplataforma para baixar vídeos usando yt-dlp.
Construído com Tauri 2.0 (Rust) e SvelteKit, fornecendo uma interface limpa e intuitiva para gerenciar downloads de vídeos.

[**English**](../README.md) | [**한국어**](README.ko.md) | [**日本語**](README.ja.md) | [**中文(简体)**](README.zh-CN.md) | [**中文(繁體)**](README.zh-TW.md) | [**Español**](README.es.md) | [**Français**](README.fr.md) | [**Deutsch**](README.de.md) | [**Русский**](README.ru.md) | [**Tiếng Việt**](README.vi.md) | **Português**

## Video

<p align="center">
  <img src="Video.gif" alt="Demonstração do yt-dlp Modern GUI" width="700">
</p>

## Recursos

- Download de vídeos e playlists com seleção de formato e qualidade
- Suporte multiplataforma (Windows, macOS, Linux)
- Fila de downloads concorrentes com cancelamento e repetição
- Histórico de downloads com pesquisa
- Interface desktop limpa para yt-dlp

<details>
<summary>Recursos avançados</summary>

- Detecção automática de dependências yt-dlp e FFmpeg com guia de instalação
- Personalização de template de nome de arquivo (modos simples e avançado)
- Suporte a cookies para conteúdo autenticado
- Detecção de downloads duplicados
- Suporte multilíngue
- 4 temas de cores (Dark, Violet, Red, Light)

</details>

> **💡 Dica:** O aplicativo configura automaticamente yt-dlp, FFmpeg e Deno na primeira execução (empacotados com o aplicativo e baixados/atualizados conforme necessário). A versão do yt-dlp gerenciada automaticamente se autoextrai a cada execução, então sua primeira inicialização pode ser lenta. Para obtenção de metadados e downloads **significativamente mais rápidos**, instale-os previamente pelo gerenciador de pacotes do seu sistema — [Homebrew](https://brew.sh/) no macOS (`brew install yt-dlp ffmpeg`), [winget](https://learn.microsoft.com/windows/package-manager/winget/) no Windows (`winget install yt-dlp.yt-dlp ffmpeg`), ou `apt`/`pacman` no Linux. Por padrão, o aplicativo detecta e prioriza as versões instaladas no PATH do sistema.

## Compilar a partir do código-fonte

### Pré-requisitos

- [Rust](https://www.rust-lang.org/tools/install) (última versão stable)
- [Node.js](https://nodejs.org/) (v18+)
- [Bun](https://bun.sh/) (gerenciador de pacotes)
- Dependências específicas da plataforma para [Tauri 2.0](https://v2.tauri.app/start/prerequisites/)

### Passos

```bash
# Clonar o repositório
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# Instalar dependências do frontend
bun install

# Executar em modo de desenvolvimento
bun run tauri dev

# Compilar para produção
bun run tauri build
```

A saída da compilação de produção estará em `src-tauri/target/release/bundle/`.

## Créditos e Licenças de Terceiros

Este aplicativo inclui ou baixa os seguintes binários de código aberto:

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg está licenciado sob a GNU General Public License v3. A versão GPL exata incluída em cada lançamento está vinculada acima, com o código-fonte correspondente disponível no projeto FFmpeg e nos provedores de compilação.

## Licença

Este projeto está licenciado sob a [Licença MIT](../LICENSE).
