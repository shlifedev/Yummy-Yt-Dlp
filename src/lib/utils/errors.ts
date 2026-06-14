import { t } from "$lib/i18n/index.svelte"

/**
 * Pull a stable i18n key (or raw string) out of an AppError value.
 * The Rust backend serializes its error variants as `{ variantName: payload }`. A unit variant
 * gives a plain string ("error.privateVideo"); a struct variant gives an object whose own fields
 * we have to dig through. We can't assume the first value is the message, so prefer an explicit
 * `message`/`msg` field, then fall back to the first string value, and finally to a JSON dump so
 * nothing is silently swallowed.
 */
export function getErrorKey(err: unknown): string {
  if (typeof err === "string") return err
  if (err && typeof err === "object") {
    const values = Object.values(err)
    if (values.length > 0) {
      const first = values[0]
      if (typeof first === "string") return first
      if (first && typeof first === "object") {
        const obj = first as Record<string, unknown>
        if (typeof obj.message === "string") return obj.message
        if (typeof obj.msg === "string") return obj.msg
        const nestedStr = Object.values(obj).find((v) => typeof v === "string")
        if (typeof nestedStr === "string") return nestedStr
      }
      try {
        return JSON.stringify(err)
      } catch {
        // fall through to unknown
      }
    }
  }
  return "error.unknown"
}

/**
 * Extract an AppError's i18n key (or raw string) and translate it to the current UI language.
 * Unexpected raw strings fall back to themselves via t() (which returns the key when no
 * translation exists).
 */
export function extractError(err: unknown): string {
  return t(getErrorKey(err))
}

export function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}
