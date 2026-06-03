<script lang="ts">
  import { commands } from "$lib/bindings"
  import type { AppSettings } from "$lib/bindings"
  import { defaultAdvancedOptions } from "$lib/advanced"
  import { onMount } from "svelte"
  import { t, setLocale, getLocale, supportedLocales } from "$lib/i18n/index.svelte"
  import { setTheme, getTheme } from "$lib/theme/index.svelte"
  import { themes, themeList, type ThemeId } from "$lib/theme/themes"

  let settings = $state<AppSettings>({
    downloadPath: "",
    defaultQuality: "1080p",
    maxConcurrent: 3,
    filenameTemplate: "%(title)s.%(ext)s",
    cookieBrowser: null,
    autoUpdateYtdlp: true,
    useAdvancedTemplate: false,
    templateUploaderFolder: false,
    templateUploadDate: false,
    templateVideoId: false,
    language: null,
    theme: null,
    minimizeToTray: null,
    depMode: "external",
    depOverrides: {},
    advanced: defaultAdvancedOptions(),
    setupCompleted: true,
  })

  let loading = $state(true)

  onMount(async () => {
    try {
      const r = await commands.getSettings()
      if (r.status === "ok") settings = r.data
    } catch (e) { console.error("Failed to load settings:", e) }
    loading = false
  })

  async function autoSave() {
    try { await commands.updateSettings(settings) }
    catch (e) { console.error("Failed to save settings:", e) }
  }

  async function handleMinimizeChange(e: Event) {
    settings.minimizeToTray = (e.target as HTMLInputElement).checked
    await autoSave()
  }

  async function handleAutoUpdateChange(e: Event) {
    settings.autoUpdateYtdlp = (e.target as HTMLInputElement).checked
    await autoSave()
  }

  async function handleLanguageChange(locale: string) {
    setLocale(locale)
    settings.language = locale
    await autoSave()
  }

  async function handleThemeChange(themeId: string) {
    setTheme(themeId as ThemeId)
    settings.theme = themeId
    await autoSave()
  }
</script>

{#if loading}
  <div class="flex justify-center py-16">
    <span class="material-symbols-outlined text-yt-primary text-3xl animate-spin">progress_activity</span>
  </div>
{:else}
  <div class="max-w-5xl mx-auto px-8 py-8 space-y-10">

    <!-- General Section -->
    <section>
      <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider mb-4 px-1">{t("settings.general")}</h3>
      <div class="bg-yt-surface border border-yt-border rounded-lg divide-y divide-yt-border/50 overflow-hidden">
         <!-- Minimize to Tray -->
         <div class="p-4 flex items-center justify-between gap-4">
            <div>
               <label for="minimize-tray" class="block text-sm font-medium text-yt-text mb-1">{t("settings.minimizeToTray")}</label>
               <p class="text-xs text-yt-text-secondary">{t("settings.minimizeToTrayDesc")}</p>
            </div>
            <label class="relative inline-flex items-center cursor-pointer">
              <input id="minimize-tray" type="checkbox" checked={settings.minimizeToTray === true} onchange={handleMinimizeChange} class="sr-only peer" />
              <div class="w-9 h-5 bg-yt-border peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-yt-primary"></div>
            </label>
         </div>
         <!-- Auto-update dependencies on launch -->
         <div class="p-4 flex items-center justify-between gap-4">
            <div>
               <label for="auto-update-deps" class="block text-sm font-medium text-yt-text mb-1">{t("settings.autoUpdateDeps")}</label>
               <p class="text-xs text-yt-text-secondary">{t("settings.autoUpdateDepsDesc")}</p>
            </div>
            <label class="relative inline-flex items-center cursor-pointer">
              <input id="auto-update-deps" type="checkbox" checked={settings.autoUpdateYtdlp === true} onchange={handleAutoUpdateChange} class="sr-only peer" />
              <div class="w-9 h-5 bg-yt-border peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-yt-primary"></div>
            </label>
         </div>
      </div>
    </section>

    <!-- Appearance -->
    <section>
      <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider mb-4 px-1">{t("settings.appearance")}</h3>
      <div class="bg-yt-surface border border-yt-border rounded-lg divide-y divide-yt-border/50 overflow-hidden">
         <!-- Language -->
         <div class="p-4 flex items-center justify-between gap-4">
            <label for="language-select" class="block text-sm font-medium text-yt-text">{t("settings.language")}</label>
            <select
              id="language-select"
              class="bg-yt-bg text-yt-text border border-yt-border rounded-md px-3 py-1.5 text-xs focus:ring-1 focus:ring-yt-primary focus:outline-none"
              value={getLocale()}
              onchange={(e) => handleLanguageChange((e.target as HTMLSelectElement).value)}
            >
              {#each supportedLocales as loc}
                <option value={loc.code}>{loc.name}</option>
              {/each}
            </select>
         </div>

         <!-- Theme -->
         <div class="p-4">
            <h4 class="block text-sm font-medium text-yt-text mb-3">{t("settings.theme")}</h4>
            <div class="grid grid-cols-4 gap-3">
              {#each themeList as themeItem}
                <button
                  class="flex flex-col items-center gap-2 p-3 rounded-lg border transition-all {getTheme() === themeItem.id ? 'border-yt-primary bg-yt-primary/5 ring-1 ring-yt-primary' : 'border-yt-border hover:bg-yt-highlight'}"
                  onclick={() => handleThemeChange(themeItem.id)}
                >
                  <div class="flex gap-1">
                    <div class="w-3 h-3 rounded-full border border-black/10" style="background-color: {themes[themeItem.id].primary}"></div>
                    <div class="w-3 h-3 rounded-full border border-black/10" style="background-color: {themes[themeItem.id].bg}"></div>
                    <div class="w-3 h-3 rounded-full border border-black/10" style="background-color: {themes[themeItem.id].surface}"></div>
                  </div>
                  <span class="text-[10px] text-yt-text font-medium">{t(themeItem.labelKey)}</span>
                </button>
              {/each}
            </div>
         </div>
      </div>
    </section>

    <!-- About / Licenses -->
    <section>
      <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider mb-4 px-1">{t("settings.about")}</h3>
      <div class="bg-yt-surface border border-yt-border rounded-lg overflow-hidden">
         <div class="p-4 space-y-3">
            <p class="text-xs text-yt-text-secondary">{t("settings.licensesDesc")}</p>
            <ul class="space-y-2 text-xs">
               <li class="flex flex-wrap items-baseline gap-x-2">
                  <span class="font-medium text-yt-text">yt-dlp</span>
                  <span class="text-yt-text-secondary">The Unlicense</span>
                  <span class="text-yt-text-secondary/70">github.com/yt-dlp/yt-dlp</span>
               </li>
               <li class="flex flex-wrap items-baseline gap-x-2">
                  <span class="font-medium text-yt-text">FFmpeg</span>
                  <span class="text-yt-text-secondary">GPL v3</span>
                  <span class="text-yt-text-secondary/70">github.com/BtbN/FFmpeg-Builds · github.com/vanloctech/ffmpeg-macos</span>
               </li>
               <li class="flex flex-wrap items-baseline gap-x-2">
                  <span class="font-medium text-yt-text">Deno</span>
                  <span class="text-yt-text-secondary">MIT</span>
                  <span class="text-yt-text-secondary/70">github.com/denoland/deno</span>
               </li>
            </ul>
            <p class="text-[10px] text-yt-text-secondary/80 leading-relaxed">{t("settings.ffmpegGplNotice")}</p>
         </div>
      </div>
    </section>

  </div>
{/if}
