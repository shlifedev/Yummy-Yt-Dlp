# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Tauri 2.0 desktop application: a yt-dlp GUI for downloading YouTube videos. SvelteKit frontend with Rust backend. Root page (`/`) redirects to `/tools/ytdlp`.

## Build Commands

```bash
# Development (runs both frontend + backend)
bun run tauri dev

# Frontend only
bun run dev               # Vite dev server
bun run check             # Svelte/TypeScript type check

# Backend only (run from repo root, not src-tauri/)
cd src-tauri && cargo check && cd ..
cd src-tauri && cargo fmt && cargo clippy && cd ..

# Run Rust tests
cd src-tauri && cargo test && cd ..

# Production build
bun run tauri build
```

## Before Committing

```bash
cd src-tauri && cargo fmt && cargo clippy && cd ..
bun run check
```

## Architecture

### Backend Module Structure (`src-tauri/src/`)

```
lib.rs              # App init, state setup (AppState, DbState, DownloadManagerState), collect_commands!/collect_events!
main.rs             # Binary entry point (delegates to lib.rs run())
modules/
  types.rs          # AppError enum (thiserror-based, 11 variants)
  logger.rs         # Logging utility (text log + structured log DB)
  log_db.rs         # SQLite structured log store (logs.db)
  log_commands.rs   # Tauri commands for querying/clearing logs
ytdlp/
  mod.rs            # Module re-exports
  types.rs          # All shared types: VideoInfo, DownloadRequest, DownloadStatus, DuplicateCheckResult, AppSettings, events, etc.
  progress.rs       # Parse yt-dlp progress output (regex-based, has unit tests)
  settings.rs       # Settings via tauri-plugin-store (settings.json)
  security.rs       # URL/IP (SSRF), output-path & filename-template, cookie-browser validation; error-message sanitization
  tray.rs           # System tray menu integration
  dep_download.rs   # Shared streamed download + SHA-256 verify; zip extract (flatten + tree/zip-slip-safe) + dir finalize helpers
  dep_ytdlp.rs      # yt-dlp install/update (onedir zip, checksum-verified, extracted to bin/ytdlp/)
  dep_ffmpeg.rs     # ffmpeg/ffprobe install
  dep_deno.rs       # deno install (used by yt-dlp for some extractors)
  binary/
    mod.rs          # Re-exports (preserves import paths)
    resolve.rs      # yt-dlp/ffmpeg binary resolution (local app dir -> system PATH fallback)
    path.rs         # PATH-aware Command construction, app bin dir, external/system mode
    dep_check.rs    # Full dependency check + cache (warmup, invalidate)
  commands/
    mod.rs          # Re-exports
    dependency.rs   # check_dependencies, install commands
    queue.rs        # download queue commands (get/retry/clear, pagination, summary)
    history.rs      # history CRUD + duplicate check commands
    settings_cmd.rs # get/update settings, select directory, available browsers
    misc.rs         # misc commands
  download/
    mod.rs          # Re-exports
    manager.rs      # DownloadManager (concurrent slots via AtomicU32 + CAS), cancel via watch::channel
    executor.rs     # execute_download (spawns yt-dlp, --no-overwrites), process_next_pending, kill_process_tree
    commands.rs     # add_to_queue, cancel_download, cancel_all_downloads, start_download (legacy)
  metadata/
    mod.rs          # Re-exports
    fetch.rs        # fetch_video_info/fetch_playlist_info via yt-dlp --dump-json (applies cookie_browser); fetch_quick_metadata (oEmbed)
    validation.rs   # validate_url (regex patterns for video/playlist/channel)
  db/
    mod.rs          # Database struct, schema + migrations (SCHEMA_VERSION), Mutex<Connection>
    queue.rs        # downloads table (queue) operations
    history.rs      # history table operations + duplicate checks (history & queue)
```

### Key Architectural Patterns

**Download Pipeline**: URL input -> `validate_url` -> `fetch_video_info`/`fetch_playlist_info` -> `check_duplicate` (history + queue) -> `add_to_queue` -> `DownloadManager.try_acquire` (CAS loop for concurrency control) -> `execute_download` (spawns yt-dlp process, `--no-overwrites`) -> `process_next_pending` (auto-dequeue)

**State Management (Rust side)**:
- `DbState = Arc<Database>` - SQLite connection wrapped in `Mutex<Connection>`
- `DownloadManagerState = Arc<DownloadManager>` - concurrent slot tracking via `AtomicU32`, cancel via `watch::channel`
- Settings stored in `tauri-plugin-store` (separate from SQLite)

**Event System**: Global events emitted via `app.emit("download-event", GlobalDownloadEvent)` for progress/completion/errors. Frontend listens via `@tauri-apps/api/event`. The `Channel<DownloadEvent>` parameter in `start_download` is legacy (unused, delegates to `add_to_queue`).

**Duplicate Detection**: `check_duplicate` returns `DuplicateCheckResult { inHistory, inQueue, historyItem }`. Single video downloads show a warning dialog allowing re-download. Batch/playlist downloads auto-skip videos already in queue.

**TypeScript Bindings**: Auto-generated by `tauri-specta` into `src/lib/bindings.ts` (DO NOT EDIT). Regenerated on `bun run tauri dev` in debug mode. All commands return `Result<T, AppError>` which becomes `{ status: "ok", data: T } | { status: "error", error: AppError }` in TypeScript.

### Frontend Structure (`src/`)

```
routes/
  +page.svelte              # Redirect to /tools/ytdlp
  +layout.svelte            # Minimal root layout (just renders children)
  tools/ytdlp/
    +layout.svelte          # App shell: top nav, dependency check/install, download queue popup, F9 debug panel, update flow
    +page.svelte            # Main page: URL input, video/playlist analysis, format/quality selection, filename template, duplicate check, download
    queue/+page.svelte      # Full download queue view
    history/+page.svelte    # Download history with search/pagination
    logs/+page.svelte       # Structured log viewer (reads logs.db)
    settings/+layout.svelte # Settings section shell (sub-nav)
    settings/+page.svelte   # Settings page (download path, concurrent downloads, cookie browser, language, theme)
    settings/dependencies/+page.svelte  # App-managed dependency (yt-dlp/ffmpeg) install/update view
```

**UI Stack**: Tailwind CSS v4, Skeleton UI v4 (`@skeletonlabs/skeleton-svelte`), Material Symbols icons. Custom CSS variables: `yt-bg`, `yt-surface`, `yt-highlight`, `yt-primary`.

## Documentation

- `README.md` (English, main) + 10 translations in `docs/`: ko, ja, zh-CN, zh-TW, es, fr, de, pt-BR, ru, vi
- When updating README content, update all 11 files to keep translations in sync

## Conventions

### Git & Pull Requests
- Write all commit messages in English (this repo overrides the global Korean-commit default — it ships an English-first README with 10 translations).
- Keep the subject line concise and imperative; use Conventional Commits prefixes (`feat:`, `fix:`, `docs:`, …).
- Do not add a `Co-authored-by` trailer.
- When asked to open a PR, write the title and body in English.

### Rust
- All Tauri commands use `#[tauri::command]` + `#[specta::specta]`
- After creating a command: add to `collect_commands![]` in `lib.rs`, run app to regenerate bindings
- Events: derive `tauri_specta::Event`, register in `collect_events![]`
- Types for frontend: `#[serde(rename_all = "camelCase")]` + `specta::Type`
- Error variants in `AppError` (modules/types.rs): use domain-specific variants, not `Custom`
- Mutex poisoning: use `unwrap_or_else(|e| e.into_inner())` pattern (see db/mod.rs, download/manager.rs)

### Svelte/TypeScript
- Svelte 5 runes: `$state()`, `$derived()`, `$effect()`, `$props()`
- No semicolons, double quotes, 2-space indent
- Import commands from `$lib/bindings`, not raw `invoke()`
- Error extraction: `Object.values(err)[0]` pattern for AppError variants
- No hardcoded user-facing strings. Any text shown in the UI must go through i18n — add the key to **all** locale files in `src/lib/i18n/locales/` (en, ko, ja, fr, de, zh-CN, zh-TW) and reference it via the translation helper. Never leave a raw Korean (or any literal) string in a component.

### Adding a New Command
1. Define in appropriate file with `#[tauri::command]` + `#[specta::specta]`
2. Add to `collect_commands![]` in `lib.rs`
3. Run `bun run tauri dev` to regenerate `src/lib/bindings.ts`
4. Use via `commands.myCommand()` in frontend

## Debugging

### App Data Directory

App identifier: `dev.shlife.yummy-yt-dlp`

| OS | Path |
|----|------|
| macOS | `~/Library/Application Support/dev.shlife.yummy-yt-dlp/` |
| Windows | `%APPDATA%/dev.shlife.yummy-yt-dlp/` |
| Linux | `~/.local/share/dev.shlife.yummy-yt-dlp/` |

### Files in App Data Directory

| File | Description |
|------|-------------|
| `log.txt` | Text log (5MB rotation, backup: `log.old.txt`) |
| `logs.db` | SQLite structured log DB |
| `ytdlp.db` | Download queue + history DB |
| `settings.json` | User settings (tauri-plugin-store) |

### Quick Log Access (macOS)

```bash
APP_DIR=~/Library/Application\ Support/dev.shlife.yummy-yt-dlp

# Read recent text logs
tail -100 "$APP_DIR/log.txt"

# Query ERROR logs from logs.db
sqlite3 "$APP_DIR/logs.db" "SELECT datetime(timestamp/1000, 'unixepoch', 'localtime'), category, message FROM logs WHERE level='ERROR' ORDER BY timestamp DESC LIMIT 20;"

# Filter by category
sqlite3 "$APP_DIR/logs.db" "SELECT datetime(timestamp/1000, 'unixepoch', 'localtime'), level, message FROM logs WHERE category='download' ORDER BY timestamp DESC LIMIT 20;"

# Check failed downloads in ytdlp.db
sqlite3 "$APP_DIR/ytdlp.db" "SELECT id, title, error_message, datetime(created_at, 'unixepoch', 'localtime') FROM downloads WHERE status='failed' ORDER BY created_at DESC LIMIT 10;"

# Read current settings
cat "$APP_DIR/settings.json"
```

### Issue-Specific Debugging

| User Report | Log Category | Additional Check |
|-------------|-------------|-----------------|
| Download fails | `download` | `ytdlp.db` downloads table `error_message` column |
| URL not recognized | `metadata` | stderr output in log details |
| yt-dlp/ffmpeg not found | `dependency` | Binary resolution logs |
| Settings not saving | `settings` | `settings.json` contents |
| App crash | `app` | Last entries in `log.txt` |

### Debugging Workflow

When a user reports an issue:
1. Read `log.txt` tail or query `logs.db` filtered by relevant category
2. Filter `level='ERROR'` to find errors
3. For download failures, check `ytdlp.db` downloads table `error_message`
4. Verify settings with `settings.json`
5. For binary resolution issues, filter `category='dependency'`

### Log Schema Reference

**logs.db — `logs` table:**
```
id        INTEGER PRIMARY KEY AUTOINCREMENT
timestamp INTEGER NOT NULL  -- Unix milliseconds (UTC)
level     TEXT NOT NULL      -- ERROR, WARN, INFO, DEBUG
category  TEXT NOT NULL      -- app, download, metadata, dependency, settings
message   TEXT NOT NULL
details   TEXT               -- nullable, extra context
```

**ytdlp.db — `downloads` table:**
```
id            INTEGER PRIMARY KEY AUTOINCREMENT
video_url     TEXT NOT NULL
video_id      TEXT NOT NULL
title         TEXT NOT NULL
format_id     TEXT NOT NULL
quality_label TEXT NOT NULL
output_path   TEXT NOT NULL
status        TEXT NOT NULL DEFAULT 'pending'  -- pending, downloading, completed, failed, cancelled
progress      REAL DEFAULT 0.0
speed         TEXT
eta           TEXT
error_message TEXT
created_at    INTEGER NOT NULL  -- Unix seconds
completed_at  INTEGER
```

**ytdlp.db — `history` table:**
```
id             INTEGER PRIMARY KEY AUTOINCREMENT
video_url      TEXT NOT NULL
video_id       TEXT NOT NULL
title          TEXT NOT NULL
quality_label  TEXT NOT NULL
format         TEXT NOT NULL
file_path      TEXT NOT NULL
file_size      INTEGER
downloaded_at  INTEGER NOT NULL  -- Unix seconds
```

**log.txt format:** `[2025-01-15 14:30:45.123] [ERROR] [download] Failed to download video`
