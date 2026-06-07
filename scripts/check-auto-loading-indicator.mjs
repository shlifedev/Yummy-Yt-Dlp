import assert from "node:assert/strict"
import { readFileSync } from "node:fs"

const page = readFileSync(new URL("../src/routes/tools/ytdlp/+page.svelte", import.meta.url), "utf8")

const countMarker = 'data-testid="playlist-count"'
assert.ok(page.includes(countMarker), "playlist header count should have a stable test marker")

const countStart = page.indexOf(countMarker)
const countBlock = page.slice(countStart, countStart + 900)
assert.match(countBlock, /autoLoading/, "playlist count block should react to autoLoading")
assert.match(countBlock, /download\.loadingMore/, "playlist count block should show the loading-more label")
assert.match(countBlock, /animate-spin/, "playlist count block should include a visible loading animation")

for (const locale of ["en", "ko", "ja", "zh-CN", "zh-TW", "fr", "de"]) {
  const localeFile = readFileSync(new URL(`../src/lib/i18n/locales/${locale}.ts`, import.meta.url), "utf8")
  assert.ok(
    localeFile.includes('"download.loadingMore"'),
    `${locale} locale should define download.loadingMore`,
  )
}
