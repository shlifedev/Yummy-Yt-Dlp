import type { AdvancedOptions } from "$lib/bindings"

// Default advanced options. MUST stay in sync with `impl Default for AdvancedOptions`
// in src-tauri/src/ytdlp/types.rs.
export function defaultAdvancedOptions(): AdvancedOptions {
  return {
    writeSubs: false,
    writeAutoSubs: false,
    embedSubs: false,
    subLangs: "en",
    convertSubs: "",
    sponsorblockMode: "off",
    sponsorblockCategories: ["sponsor"],
    embedThumbnail: false,
    embedMetadata: false,
    embedChapters: false,
    writeThumbnail: false,
    writeInfoJson: false,
    videoCodec: "auto",
    limitRate: "",
    concurrentFragments: 1,
    retries: null,
    sleepInterval: 0,
    mergeOutputFormat: "",
    remuxVideo: "",
    downloadSections: "",
    splitChapters: false,
    proxy: "",
    noMtime: false,
    restrictFilenames: false,
  }
}

// Select option metadata shared between the UI and validation.
export const VIDEO_CODECS = ["auto", "av01", "vp9", "h264"] as const
export const SPONSORBLOCK_MODES = ["off", "mark", "remove"] as const
export const SPONSORBLOCK_CATEGORIES = [
  "sponsor",
  "intro",
  "outro",
  "selfpromo",
  "preview",
  "filler",
  "interaction",
  "music_offtopic",
] as const
export const CONTAINER_FORMATS = ["", "mp4", "mkv", "webm"] as const
export const SUB_CONVERT_FORMATS = ["", "srt", "ass", "vtt", "lrc"] as const

// Inline validation regexes. MUST mirror the sanitize_* helpers in
// src-tauri/src/ytdlp/security.rs so the UI rejects the same inputs the backend would.
const RE = {
  subLangs: /^[A-Za-z0-9,.\-_*]+$/,
  limitRate: /^\d+(\.\d+)?[KMGkmg]?$/,
  downloadSections: /^\*?\d{1,2}:\d{2}(:\d{2})?-\d{1,2}:\d{2}(:\d{2})?$/,
  proxy: /^(https?|socks4|socks5):\/\/[A-Za-z0-9.\-_]+(:\d{1,5})?\/?$/i,
} as const

export type AdvancedTextField = keyof typeof RE

// An empty free-text field means "unset" and is always valid. Otherwise it must match the regex.
export function validateAdvancedField(field: AdvancedTextField, value: string): boolean {
  const v = value.trim()
  if (v === "") return true
  if (field === "subLangs" && v.length > 200) return false
  return RE[field].test(v)
}
