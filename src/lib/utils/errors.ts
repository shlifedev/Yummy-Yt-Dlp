import { t } from "$lib/i18n/index.svelte"

/**
 * Extract an AppError's i18n key (or raw string) and translate it to the current UI language.
 * The Rust backend returns stable keys like "error.privateVideo"; any unexpected raw string
 * falls back to itself via t() (which returns the key when no translation exists).
 */
export function extractError(err: unknown): string {
  let key = "error.unknown"
  if (typeof err === "string") key = err
  else if (err && typeof err === "object") {
    const values = Object.values(err)
    if (values.length > 0 && typeof values[0] === "string") key = values[0]
  }
  return t(key)
}
