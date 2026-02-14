<script lang="ts">
  import { commands } from "$lib/bindings"
  import { onMount } from "svelte"
  import { t, setLocale, getLocale, supportedLocales } from "$lib/i18n/index.svelte"
  import { setTheme, getTheme } from "$lib/theme/index.svelte"
  import { themes, themeList, type ThemeId } from "$lib/theme/themes"

  let settings = $state({
    downloadPath: "",
    defaultQuality: "1080p",
    maxConcurrent: 3,
    filenameTemplate: "%(title)s.%(ext)s",
    cookieBrowser: null as string | null,
    autoUpdateYtdlp: true,
    useAdvancedTemplate: false,
    templateUploaderFolder: false,
    templateUploadDate: false,
    templateVideoId: false,
    language: null as string | null,
    theme: null as string | null,
    minimizeToTray: null as boolean | null,
  })

  let browsers = $state<string[]>([])
  let loading = $state(true)
  let saving = $state(false)
  let saved = $state(false)
  let updateStatus = $state("")

  // 4-3: Separate try/catch for getSettings and getAvailableBrowsers
  onMount(async () => {
    try {
      const r = await commands.getSettings()
      if (r.status === "ok") settings = r.data
    } catch (e) { console.error("Failed to load settings:", e) }
    try {
      browsers = await commands.getAvailableBrowsers()
    } catch (e) { console.error("Failed to load browsers:", e) }
    loading = false
  })

  async function handleSave() {
    saving = true; saved = false
    try {
      const r = await commands.updateSettings(settings)
      if (r.status === "ok") { saved = true; setTimeout(() => saved = false, 2000) }
    } catch (e) { console.error(e) }
    finally { saving = false }
  }

  async function handleSelectDir() {
    try {
      const r = await commands.selectDownloadDirectory()
      if (r.status === "ok" && r.data) settings.downloadPath = r.data
    } catch (e) { console.error("Failed to select directory:", e) }
  }

  async function handleUpdateYtdlp() {
    updateStatus = t("settings.updating")
    try {
      const r = await commands.updateYtdlp()
      updateStatus = r.status === "ok" ? r.data : t("settings.updateFailed")
    } catch (e: any) { updateStatus = "실패: " + (e.message || e) }
  }

  async function handleLanguageChange(locale: string) {
    setLocale(locale)
    settings.language = locale
    // Auto-save so the change persists without requiring the user to click Save
    await handleSave()
  }

  async function handleThemeChange(themeId: string) {
    setTheme(themeId as ThemeId)
    settings.theme = themeId
    // Auto-save so the change persists without requiring the user to click Save
    await handleSave()
  }
</script>

<div class="flex-1 flex flex-col h-full overflow-y-auto hide-scrollbar">
  <header class="px-6 py-4 shrink-0">
    <h2 class="text-xl font-display font-bold text-gray-100">{t("settings.title")}</h2>
    <p class="text-gray-400 mt-1">{t("settings.subtitle")}</p>
  </header>

  {#if loading}
    <div class="flex justify-center py-16">
      <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
    </div>
  {:else}
    <div class="px-6 pb-6 space-y-4 max-w-2xl">
      <!-- Download Path -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-white/[0.06]">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-blue-500/10 rounded-lg text-blue-600">
            <span class="material-symbols-outlined text-[20px]">folder</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-100">{t("settings.downloadPath")}</h3>
        </div>
        <div class="flex gap-2">
          <input
            type="text"
            class="flex-1 h-11 bg-yt-surface text-gray-100 rounded-xl px-4 border border-white/[0.06] focus:ring-2 focus:ring-yt-primary focus:outline-none text-sm font-mono"
            bind:value={settings.downloadPath}
            readonly
          />
          <button class="h-11 px-5 rounded-xl bg-yt-surface hover:bg-white/[0.06] text-gray-400 text-sm font-medium transition-colors border border-white/[0.06]" onclick={handleSelectDir}>
            {t("settings.browse")}
          </button>
        </div>
      </div>

      <!-- Concurrent Downloads -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-white/[0.06]">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-amber-500/10 rounded-lg text-amber-600">
            <span class="material-symbols-outlined text-[20px]">speed</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-100">{t("settings.concurrent")}</h3>
        </div>
        <div class="flex items-center gap-4">
          <input type="range" class="flex-1 accent-yt-primary" min="1" max="10" bind:value={settings.maxConcurrent} />
          <span class="text-lg font-bold font-mono w-8 text-center text-gray-100">{settings.maxConcurrent}</span>
        </div>
      </div>

      <!-- Cookie Browser -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-white/[0.06]">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-emerald-500/10 rounded-lg text-emerald-600">
            <span class="material-symbols-outlined text-[20px]">cookie</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-100">{t("settings.cookieBrowser")}</h3>
        </div>
        <div class="relative">
          <select
            class="w-full bg-yt-surface text-gray-100 border border-white/[0.06] rounded-xl px-4 py-2.5 focus:ring-2 focus:ring-yt-primary focus:outline-none appearance-none cursor-pointer"
            bind:value={settings.cookieBrowser}
          >
            <option value={null}>{t("settings.none")}</option>
            {#each browsers as browser}
              <option value={browser}>{browser}</option>
            {/each}
          </select>
          <div class="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-gray-400">
            <span class="material-symbols-outlined text-[20px]">expand_more</span>
          </div>
        </div>
        <p class="text-xs text-gray-400 mt-2">{t("settings.cookieHelp")}</p>
      </div>

      <!-- Auto Update -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-white/[0.06]">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-cyan-500/10 rounded-lg text-cyan-600">
            <span class="material-symbols-outlined text-[20px]">update</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-100">{t("settings.autoUpdate")}</h3>
        </div>
        <div class="flex items-center justify-between bg-yt-surface p-2.5 rounded-xl px-4 border border-white/[0.06]">
          <span class="text-sm text-gray-400">{t("settings.autoUpdateDesc")}</span>
          <label class="relative inline-flex items-center cursor-pointer">
            <input type="checkbox" bind:checked={settings.autoUpdateYtdlp} class="sr-only peer" />
            <div class="w-9 h-5 bg-white/10 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-yt-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-white/10 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-yt-primary"></div>
          </label>
        </div>

        <div class="flex items-center gap-3 mt-4">
          <button
            class="px-5 py-2 rounded-xl bg-yt-surface hover:bg-white/[0.06] text-gray-400 text-sm font-medium transition-colors border border-white/[0.06]"
            onclick={handleUpdateYtdlp}
          >
            <span class="material-symbols-outlined text-[18px] align-middle mr-1">refresh</span>
            {t("settings.updateNow")}
          </button>
          {#if updateStatus}
            <span class="text-sm text-gray-400">{updateStatus}</span>
          {/if}
        </div>
      </div>

      <!-- Minimize to Tray -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-white/[0.06]">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-purple-500/10 rounded-lg text-purple-600">
            <span class="material-symbols-outlined text-[20px]">minimize</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-100">{t("settings.minimizeToTray")}</h3>
        </div>
        <div class="flex items-center justify-between bg-yt-surface p-2.5 rounded-xl px-4 border border-white/[0.06]">
          <span class="text-sm text-gray-400">{t("settings.minimizeToTrayDesc")}</span>
          <label class="relative inline-flex items-center cursor-pointer">
            <input type="checkbox" checked={settings.minimizeToTray === true} onchange={(e) => settings.minimizeToTray = (e.target as HTMLInputElement).checked} class="sr-only peer" />
            <div class="w-9 h-5 bg-white/10 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-yt-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-white/10 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-yt-primary"></div>
          </label>
        </div>
      </div>

      <!-- Language -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-white/[0.06]">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-indigo-500/10 rounded-lg text-indigo-600">
            <span class="material-symbols-outlined text-[20px]">language</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-100">{t("settings.language")}</h3>
        </div>
        <div class="relative">
          <select
            class="w-full bg-yt-surface text-gray-100 border border-white/[0.06] rounded-xl px-4 py-2.5 focus:ring-2 focus:ring-yt-primary focus:outline-none appearance-none cursor-pointer"
            value={getLocale()}
            onchange={(e) => handleLanguageChange((e.target as HTMLSelectElement).value)}
          >
            {#each supportedLocales as loc}
              <option value={loc.code}>{loc.name}</option>
            {/each}
          </select>
          <div class="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-gray-400">
            <span class="material-symbols-outlined text-[20px]">expand_more</span>
          </div>
        </div>
      </div>

      <!-- Theme -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-white/[0.06]">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-pink-500/10 rounded-lg text-pink-600">
            <span class="material-symbols-outlined text-[20px]">palette</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-100">{t("settings.theme")}</h3>
        </div>
        <div class="grid grid-cols-4 gap-2">
          {#each themeList as themeItem}
            <button
              class="flex flex-col items-center gap-2 p-3 rounded-xl border transition-all {getTheme() === themeItem.id ? 'border-yt-primary bg-yt-primary/10' : 'border-white/[0.06] hover:border-white/[0.12]'}"
              onclick={() => handleThemeChange(themeItem.id)}
            >
              <div class="flex gap-1">
                <div class="w-4 h-4 rounded-full" style="background-color: {themes[themeItem.id].primary}"></div>
                <div class="w-4 h-4 rounded-full" style="background-color: {themes[themeItem.id].bg}"></div>
                <div class="w-4 h-4 rounded-full" style="background-color: {themes[themeItem.id].surface}"></div>
              </div>
              <span class="text-xs text-gray-400 font-medium">{t(themeItem.labelKey)}</span>
            </button>
          {/each}
        </div>
      </div>

      <!-- Save Button -->
      <button
        class="w-full group relative overflow-hidden rounded-xl bg-gradient-to-r from-yt-primary to-blue-600 p-[1px] disabled:opacity-50"
        onclick={handleSave}
        disabled={saving}
      >
        <div class="relative h-11 bg-yt-surface group-hover:bg-opacity-0 transition-all rounded-xl flex items-center justify-center gap-3">
          <div class="absolute inset-0 bg-gradient-to-r from-yt-primary to-blue-600 opacity-20 group-hover:opacity-100 transition-opacity duration-300 rounded-xl"></div>
          {#if saving}
            <span class="material-symbols-outlined text-white z-10 animate-spin">progress_activity</span>
            <span class="text-sm font-semibold text-white z-10 font-display">{t("settings.saving")}</span>
          {:else if saved}
            <span class="material-symbols-outlined text-white z-10">check_circle</span>
            <span class="text-sm font-semibold text-white z-10 font-display">{t("settings.saved")}</span>
          {:else}
            <span class="material-symbols-outlined text-white z-10">save</span>
            <span class="text-sm font-semibold text-white z-10 font-display">{t("settings.save")}</span>
          {/if}
        </div>
      </button>
    </div>
  {/if}
</div>
