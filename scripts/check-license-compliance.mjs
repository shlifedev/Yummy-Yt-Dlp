import assert from "node:assert/strict"
import { existsSync, readFileSync } from "node:fs"

const read = (path) => readFileSync(new URL(`../${path}`, import.meta.url), "utf8")

const readmes = [
  "README.md",
  "docs/README.ko.md",
  "docs/README.ja.md",
  "docs/README.zh-CN.md",
  "docs/README.zh-TW.md",
  "docs/README.fr.md",
  "docs/README.de.md",
  "docs/README.es.md",
  "docs/README.pt-BR.md",
  "docs/README.ru.md",
  "docs/README.vi.md",
]

for (const path of readmes) {
  const content = read(path)
  assert.ok(
    !content.includes("vanloctech/ffmpeg-macos"),
    `${path} should not reference the nonfree macOS FFmpeg build`,
  )
}

const tauriConfig = JSON.parse(read("src-tauri/tauri.conf.json"))
assert.equal(
  tauriConfig.bundle.resources["third_party_licenses/*"],
  "third_party_licenses/",
  "Tauri bundle should include third-party license resources",
)

for (const path of [
  "src-tauri/third_party_licenses/THIRD_PARTY_NOTICES.md",
  "src-tauri/third_party_licenses/GPL-3.0.txt",
  "src-tauri/third_party_licenses/DENO-LICENSE.txt",
]) {
  assert.ok(existsSync(new URL(`../${path}`, import.meta.url)), `${path} should exist`)
}

const mainReadme = read("README.md")
assert.match(
  mainReadme,
  /yt-dlp[\s\S]*standalone release binaries[\s\S]*GPLv3\+/i,
  "README should distinguish yt-dlp source license from standalone binary license",
)
assert.match(
  mainReadme,
  /macOS[\s\S]*FFmpeg[\s\S]*auto-download[\s\S]*disabled/i,
  "README should explain the macOS FFmpeg auto-download compliance change",
)

const settingsPage = read("src/routes/tools/ytdlp/settings/+page.svelte")
assert.ok(
  settingsPage.includes("standalone binaries: GPLv3+"),
  "About screen should disclose yt-dlp standalone binary licensing",
)
assert.ok(
  !settingsPage.includes("vanloctech/ffmpeg-macos"),
  "About screen should not point users to the nonfree macOS FFmpeg build",
)
