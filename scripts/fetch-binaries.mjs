// Populate src-tauri/binaries/ with the sidecar binaries (yt-dlp onedir,
// ffmpeg, ffprobe, deno) so `tauri dev` can seed them like a release build.
// Mirrors the download steps in .github/workflows/build-release.yml, but only
// for the host platform/arch (no universal lipo). Skips anything already
// present, so it adds no overhead to subsequent dev runs.
import { spawnSync } from "node:child_process"
import { mkdirSync, existsSync, rmSync, cpSync, renameSync, chmodSync, mkdtempSync, writeFileSync, readFileSync, readdirSync, statSync } from "node:fs"
import { tmpdir } from "node:os"
import { join, dirname } from "node:path"
import { fileURLToPath } from "node:url"

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..")
const BIN = join(repoRoot, "src-tauri", "binaries")

const platform = process.platform
const arch = process.arch

if (!["darwin", "win32", "linux"].includes(platform)) {
  console.log(`fetch-binaries: ${platform} is not a bundled-release target; skipping (use the in-app installer instead)`)
  process.exit(0)
}

const isWin = platform === "win32"
const isLinux = platform === "linux"
const exe = (name) => (isWin ? `${name}.exe` : name)

const targets = {
  ytdlp: join(BIN, "ytdlp", exe("yt-dlp")),
  ffmpeg: join(BIN, exe("ffmpeg")),
  ffprobe: join(BIN, exe("ffprobe")),
  deno: join(BIN, exe("deno")),
}

if (Object.values(targets).every(existsSync)) {
  console.log("fetch-binaries: all sidecar binaries already present, skipping")
  process.exit(0)
}

mkdirSync(BIN, { recursive: true })
const tmp = mkdtempSync(join(tmpdir(), "yummy-binaries-"))

async function download(url, dest) {
  console.log(`fetch-binaries: downloading ${url}`)
  const res = await fetch(url, { redirect: "follow" })
  if (!res.ok) throw new Error(`download failed (${res.status}) for ${url}`)
  writeFileSync(dest, Buffer.from(await res.arrayBuffer()))
}

// bsdtar (default tar on macOS and Windows 10+) extracts both zip and tar.gz.
// Linux ships GNU tar, which can't read zip — use unzip there instead.
function extract(archive, destDir) {
  mkdirSync(destDir, { recursive: true })
  const useUnzip = isLinux && archive.endsWith(".zip")
  const r = useUnzip
    ? spawnSync("unzip", ["-q", archive, "-d", destDir], { stdio: "inherit" })
    : spawnSync("tar", ["-xf", archive, "-C", destDir], { stdio: "inherit" })
  if (r.status !== 0) throw new Error(`extraction failed for ${archive} (linux needs unzip + xz-utils installed)`)
}

function findFile(dir, name) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry)
    if (statSync(p).isDirectory()) {
      const found = findFile(p, name)
      if (found) return found
    } else if (entry === name) {
      return p
    }
  }
  return null
}

function makeExecutable(path) {
  if (!isWin) chmodSync(path, 0o755)
}

async function fetchYtdlp() {
  const zipName = isWin ? "yt-dlp_win.zip" : isLinux ? "yt-dlp_linux.zip" : "yt-dlp_macos.zip"
  const archive = join(tmp, zipName)
  await download(`https://github.com/yt-dlp/yt-dlp/releases/latest/download/${zipName}`, archive)
  const extractDir = join(tmp, "ytdlp-extract")
  extract(archive, extractDir)

  const destDir = join(BIN, "ytdlp")
  // .gitkeep is tracked (it keeps the dir present for build-time resource
  // validation) — preserve it across the wipe.
  const gitkeep = join(destDir, ".gitkeep")
  const keep = existsSync(gitkeep) ? readFileSync(gitkeep) : null
  rmSync(destDir, { recursive: true, force: true })
  mkdirSync(destDir, { recursive: true })
  cpSync(extractDir, destDir, { recursive: true })
  if (keep !== null) writeFileSync(gitkeep, keep)

  // The macOS/Linux onedir zips name the executable yt-dlp_macos / yt-dlp_linux;
  // normalize to yt-dlp (same rule as dep_ytdlp.rs get_archived_exe_name()).
  if (!isWin) {
    const archivedName = join(destDir, isLinux ? "yt-dlp_linux" : "yt-dlp_macos")
    if (existsSync(archivedName)) renameSync(archivedName, targets.ytdlp)
  }
  // Fail loudly if the upstream onedir layout ever changes, so we never seed a
  // tree that can't resolve yt-dlp.
  if (!existsSync(targets.ytdlp) || !existsSync(join(destDir, "_internal"))) {
    throw new Error("yt-dlp onedir layout unexpected: missing executable or _internal/")
  }
  makeExecutable(targets.ytdlp)
}

async function fetchFfmpeg() {
  const extractDir = join(tmp, "ffx")
  if (isWin) {
    const archive = join(tmp, "ffmpeg.zip")
    await download("https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip", archive)
    extract(archive, extractDir)
  } else if (isLinux) {
    // Same BtbN builds dep_ffmpeg.rs uses at runtime. Needs xz-utils for tar to unpack.
    const ffArch = arch === "arm64" ? "linuxarm64" : "linux64"
    const archive = join(tmp, "ffmpeg.tar.xz")
    await download(`https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-${ffArch}-gpl.tar.xz`, archive)
    extract(archive, extractDir)
  } else {
    const ffArch = arch === "arm64" ? "arm64" : "x64"
    const archive = join(tmp, "ffmpeg.tar.gz")
    await download(`https://github.com/vanloctech/ffmpeg-macos/releases/latest/download/ffmpeg-macos-${ffArch}.tar.gz`, archive)
    extract(archive, extractDir)
  }
  for (const name of ["ffmpeg", "ffprobe"]) {
    const found = findFile(extractDir, exe(name))
    if (!found) {
      // ffprobe is optional in CI too; only ffmpeg is required.
      if (name === "ffmpeg") throw new Error("ffmpeg not found in archive")
      continue
    }
    cpSync(found, targets[name])
    makeExecutable(targets[name])
  }
}

async function fetchDeno() {
  const denoTarget = isWin
    ? "x86_64-pc-windows-msvc"
    : isLinux
      ? arch === "arm64"
        ? "aarch64-unknown-linux-gnu"
        : "x86_64-unknown-linux-gnu"
      : arch === "arm64"
        ? "aarch64-apple-darwin"
        : "x86_64-apple-darwin"
  const archive = join(tmp, "deno.zip")
  await download(`https://github.com/denoland/deno/releases/latest/download/deno-${denoTarget}.zip`, archive)
  const extractDir = join(tmp, "denox")
  extract(archive, extractDir)
  const found = findFile(extractDir, exe("deno"))
  if (!found) throw new Error("deno not found in archive")
  cpSync(found, targets.deno)
  makeExecutable(targets.deno)
}

try {
  if (!existsSync(targets.ytdlp)) await fetchYtdlp()
  if (!existsSync(targets.ffmpeg)) await fetchFfmpeg()
  if (!existsSync(targets.deno)) await fetchDeno()
  console.log("fetch-binaries: done")
} finally {
  rmSync(tmp, { recursive: true, force: true })
}
