import { locale as getOsLocale } from "@tauri-apps/plugin-os"
import en from "./locales/en"
import ko from "./locales/ko"
import ja from "./locales/ja"
import zhCN from "./locales/zh-CN"
import zhTW from "./locales/zh-TW"
import fr from "./locales/fr"
import de from "./locales/de"

type Messages = Record<string, string>

const locales: Record<string, Messages> = {
  en, ko, ja, "zh-CN": zhCN, "zh-TW": zhTW, fr, de,
}

export const supportedLocales = [
  { code: "en", name: "English" },
  { code: "ko", name: "한국어" },
  { code: "ja", name: "日本語" },
  { code: "zh-CN", name: "简体中文" },
  { code: "zh-TW", name: "繁體中文" },
  { code: "fr", name: "Français" },
  { code: "de", name: "Deutsch" },
]

let currentLocale = $state("en")

export function t(key: string, params?: Record<string, string | number>): string {
  const msgs = locales[currentLocale] || locales.en
  let msg = msgs[key] || locales.en[key] || key
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      msg = msg.replaceAll(`{${k}}`, String(v))
    }
  }
  return msg
}

export function getLocale(): string {
  return currentLocale
}

export function getDateLocale(): string {
  const map: Record<string, string> = {
    en: "en-US", ko: "ko-KR", ja: "ja-JP",
    "zh-CN": "zh-CN", "zh-TW": "zh-TW", fr: "fr-FR", de: "de-DE",
  }
  return map[currentLocale] || "en-US"
}

// Keep the document language in sync with the UI language so the <html lang> attribute
// (hardcoded in app.html for first paint) reflects the actual locale.
function applyDocumentLang() {
  if (typeof document !== "undefined") {
    document.documentElement.lang = currentLocale
  }
}

export function setLocale(locale: string) {
  if (locales[locale]) {
    currentLocale = locale
    applyDocumentLang()
  }
}

export async function initLocale(savedLocale?: string | null) {
  if (savedLocale && locales[savedLocale]) {
    currentLocale = savedLocale
    applyDocumentLang()
    return
  }
  try {
    const sysLocale = await getOsLocale()
    if (sysLocale) {
      if (sysLocale.startsWith("zh")) {
        currentLocale = (sysLocale.includes("TW") || sysLocale.includes("Hant")) ? "zh-TW" : "zh-CN"
      } else {
        const lang = sysLocale.split("-")[0].split("_")[0]
        if (locales[lang]) currentLocale = lang
      }
    }
  } catch (e) {
    console.error("Failed to detect system locale:", e)
  }
  applyDocumentLang()
}
