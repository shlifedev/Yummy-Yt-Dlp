# yt-dlp Modern GUI


Une application de bureau moderne et multiplateforme pour télécharger des vidéos avec yt-dlp.
Construite avec Tauri 2.0 (Rust) et SvelteKit, offrant une interface propre et intuitive pour gérer les téléchargements de vidéos.

[**한국어**](README.ko.md) | [**日本語**](README.ja.md) | [**中文(简体)**](README.zh-CN.md) | [**中文(繁體)**](README.zh-TW.md) | [**Español**](README.es.md) | **Français** | [**Deutsch**](README.de.md) | [**Português**](README.pt-BR.md) | [**Русский**](README.ru.md) | [**Tiếng Việt**](README.vi.md)

## Fonctionnalités

- Téléchargement de vidéos et de listes de lecture avec sélection du format et de la qualité
- Support multiplateforme (Windows, macOS, Linux)
- File d'attente concurrente avec annulation et nouvelle tentative
- Historique de téléchargement consultable
- Interface de bureau claire pour yt-dlp

<details>
<summary>Fonctionnalités avancées</summary>

- Détection automatique des dépendances yt-dlp et FFmpeg avec guide d'installation
- Personnalisation du modèle de nom de fichier (modes simple et avancé)
- Support des cookies pour les contenus authentifiés
- Détection des téléchargements en double
- Support multilingue
- 4 thèmes de couleurs (Dark, Violet, Red, Light)

</details>

> **💡 Astuce :** L'application configure automatiquement yt-dlp, FFmpeg et Deno au premier lancement (fournis avec l'application et téléchargés/mis à jour selon les besoins). La version de yt-dlp gérée automatiquement se décompresse à chaque exécution, ce qui peut ralentir son premier démarrage. Pour une récupération des métadonnées et des téléchargements **nettement plus rapides**, installez-les au préalable via votre gestionnaire de paquets — [Homebrew](https://brew.sh/) sur macOS (`brew install yt-dlp ffmpeg`), [winget](https://learn.microsoft.com/windows/package-manager/winget/) sur Windows (`winget install yt-dlp.yt-dlp ffmpeg`), ou `apt`/`pacman` sur Linux. Par défaut, l'application détecte et privilégie les versions installées sur le système depuis votre PATH.

## Compiler depuis les sources

### Prérequis

- [Rust](https://www.rust-lang.org/tools/install) (dernière version stable)
- [Node.js](https://nodejs.org/) (v18+)
- [Bun](https://bun.sh/) (gestionnaire de paquets)
- Dépendances spécifiques à la plateforme pour [Tauri 2.0](https://v2.tauri.app/start/prerequisites/)

### Étapes

```bash
# Cloner le dépôt
git clone https://github.com/shlifedev/yt-dlp-modern-gui.git
cd yt-dlp-modern-gui

# Installer les dépendances frontend
bun install

# Exécuter en mode développement
bun run tauri dev

# Compiler pour la production
bun run tauri build
```

Le résultat de la compilation se trouve dans `src-tauri/target/release/bundle/`.

## Feuille de route

1. Application de téléchargement pour les utilisateurs mobiles (vous pouvez héberger votre propre serveur yt-dlp)
2. Mise à jour automatique des versions

## Crédits et Licences Tierces

Cette application intègre ou télécharge les binaires open source suivants :

- **yt-dlp** — The Unlicense — https://github.com/yt-dlp/yt-dlp
- **FFmpeg** — GPLv3 — bundled GPL builds: https://github.com/BtbN/FFmpeg-Builds (Windows/Linux), https://github.com/vanloctech/ffmpeg-macos (macOS); source: https://ffmpeg.org
- **Deno** — MIT — https://github.com/denoland/deno

FFmpeg est distribué sous GNU General Public License v3. La version GPL exacte fournie avec chaque version est liée ci-dessus, avec le code source correspondant disponible auprès du projet FFmpeg et des fournisseurs de compilation.

## Licence

Ce projet est sous licence [MIT License](../LICENSE).
