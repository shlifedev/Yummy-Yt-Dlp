<script lang="ts">
  import { commands } from "$lib/bindings"
  import { onMount } from "svelte"

  let settings = $state({
    downloadPath: "",
    defaultQuality: "1080p",
    maxConcurrent: 3,
    filenameTemplate: "%(title)s.%(ext)s",
    cookieBrowser: null as string | null,
    autoUpdateYtdlp: true,
  })

  let browsers = $state<string[]>([])
  let loading = $state(true)
  let saving = $state(false)
  let saved = $state(false)
  let updateStatus = $state("")

  onMount(async () => {
    try {
      const r = await commands.getSettings()
      if (r.status === "ok") settings = r.data
      browsers = await commands.getAvailableBrowsers()
    } catch (e) { console.error(e) }
    finally { loading = false }
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
    } catch {}
  }

  async function handleUpdateYtdlp() {
    updateStatus = "업데이트 중..."
    try {
      const r = await commands.updateYtdlp()
      updateStatus = r.status === "ok" ? r.data : "업데이트 실패"
    } catch (e: any) { updateStatus = "실패: " + (e.message || e) }
  }
</script>

<div class="flex-1 flex flex-col h-full overflow-y-auto hide-scrollbar">
  <header class="px-6 py-4 shrink-0">
    <h2 class="text-xl font-display font-bold text-gray-900">Settings</h2>
    <p class="text-gray-400 mt-1">Configure your download preferences</p>
  </header>

  {#if loading}
    <div class="flex justify-center py-16">
      <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
    </div>
  {:else}
    <div class="px-6 pb-6 space-y-4 max-w-2xl">
      <!-- Download Path -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-blue-500/10 rounded-lg text-blue-600">
            <span class="material-symbols-outlined text-[20px]">folder</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-900">Download Path</h3>
        </div>
        <div class="flex gap-2">
          <input
            type="text"
            class="flex-1 h-11 bg-yt-surface text-gray-900 rounded-xl px-4 border border-gray-200 focus:ring-2 focus:ring-yt-primary focus:outline-none text-sm font-mono"
            bind:value={settings.downloadPath}
            readonly
          />
          <button class="h-11 px-5 rounded-xl bg-yt-surface hover:bg-gray-100 text-gray-600 text-sm font-medium transition-colors border border-gray-200" onclick={handleSelectDir}>
            Browse
          </button>
        </div>
      </div>

      <!-- Filename Template -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-purple-500/10 rounded-lg text-purple-600">
            <span class="material-symbols-outlined text-[20px]">edit_note</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-900">Filename Template</h3>
        </div>
        <input
          type="text"
          class="w-full h-11 bg-yt-surface text-gray-900 rounded-xl px-4 border border-gray-200 focus:ring-2 focus:ring-yt-primary focus:outline-none text-sm font-mono"
          bind:value={settings.filenameTemplate}
        />
        <p class="text-xs text-gray-400 mt-2">Variables: %(title)s, %(id)s, %(ext)s, %(uploader)s, %(upload_date)s</p>
      </div>

      <!-- Concurrent Downloads -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-amber-500/10 rounded-lg text-amber-600">
            <span class="material-symbols-outlined text-[20px]">speed</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-900">Concurrent Downloads</h3>
        </div>
        <div class="flex items-center gap-4">
          <input type="range" class="flex-1 accent-yt-primary" min="1" max="10" bind:value={settings.maxConcurrent} />
          <span class="text-lg font-bold font-mono w-8 text-center text-gray-900">{settings.maxConcurrent}</span>
        </div>
      </div>

      <!-- Cookie Browser -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-emerald-500/10 rounded-lg text-emerald-600">
            <span class="material-symbols-outlined text-[20px]">cookie</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-900">Cookie Browser</h3>
        </div>
        <div class="relative">
          <select
            class="w-full bg-yt-surface text-gray-900 border border-gray-200 rounded-xl px-4 py-2.5 focus:ring-2 focus:ring-yt-primary focus:outline-none appearance-none cursor-pointer"
            bind:value={settings.cookieBrowser}
          >
            <option value={null}>None</option>
            {#each browsers as browser}
              <option value={browser}>{browser}</option>
            {/each}
          </select>
          <div class="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-gray-400">
            <span class="material-symbols-outlined text-[20px]">expand_more</span>
          </div>
        </div>
        <p class="text-xs text-gray-400 mt-2">로그인이 필요한 콘텐츠를 다운로드할 때 사용</p>
      </div>

      <!-- Auto Update -->
      <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
        <div class="flex items-center gap-3 mb-3">
          <div class="p-2 bg-cyan-500/10 rounded-lg text-cyan-600">
            <span class="material-symbols-outlined text-[20px]">update</span>
          </div>
          <h3 class="font-display font-semibold text-base text-gray-900">Auto Update</h3>
        </div>
        <div class="flex items-center justify-between bg-yt-surface p-2.5 rounded-xl px-4 border border-gray-200">
          <span class="text-sm text-gray-600">Auto-update yt-dlp on launch</span>
          <label class="relative inline-flex items-center cursor-pointer">
            <input type="checkbox" bind:checked={settings.autoUpdateYtdlp} class="sr-only peer" />
            <div class="w-9 h-5 bg-gray-300 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-yt-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-yt-primary"></div>
          </label>
        </div>

        <div class="flex items-center gap-3 mt-4">
          <button
            class="px-5 py-2 rounded-xl bg-yt-surface hover:bg-gray-100 text-gray-600 text-sm font-medium transition-colors border border-gray-200"
            onclick={handleUpdateYtdlp}
          >
            <span class="material-symbols-outlined text-[18px] align-middle mr-1">refresh</span>
            Update Now
          </button>
          {#if updateStatus}
            <span class="text-sm text-gray-500">{updateStatus}</span>
          {/if}
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
            <span class="text-sm font-semibold text-white z-10 font-display">Saving...</span>
          {:else if saved}
            <span class="material-symbols-outlined text-white z-10">check_circle</span>
            <span class="text-sm font-semibold text-white z-10 font-display">Saved!</span>
          {:else}
            <span class="material-symbols-outlined text-white z-10">save</span>
            <span class="text-sm font-semibold text-white z-10 font-display">Save Settings</span>
          {/if}
        </div>
      </button>
    </div>
  {/if}
</div>
