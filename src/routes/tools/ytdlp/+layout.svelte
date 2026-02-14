<script lang="ts">
  import { commands } from "$lib/bindings"
  import { listen } from "@tauri-apps/api/event"
  import { platform } from "@tauri-apps/plugin-os"
  import { page } from "$app/stores"
  import { onMount, onDestroy } from "svelte"
  import { t, initLocale } from "$lib/i18n/index.svelte"
  import { initTheme } from "$lib/theme/index.svelte"

  let { children } = $props()

  let checking = $state(true)
  let depsInstalled = $state(false)
  let ytdlpInstalled = $state(false)
  let ytdlpVersion = $state<string | null>(null)
  let ffmpegInstalled = $state(false)
  let ffmpegVersion = $state<string | null>(null)
  let ytdlpDebug = $state("")
  let showDebug = $state(false)
  let currentPlatform = $state<string>("macos")
  let copiedCmd = $state<string | null>(null)

  // Popup state
  let popupOpen = $state(false)
  let activeDownloads = $state<any[]>([])
  let recentCompleted = $state<any[]>([])
  let activeCount = $derived(activeDownloads.filter(d => d.status === "downloading").length)
  let pendingCount = $derived(activeDownloads.filter(d => d.status === "pending").length)

  // Toast state
  let toastMessage = $state("")
  let toastVisible = $state(false)
  let toastIcon = $state("check_circle")
  let toastTimeout: ReturnType<typeof setTimeout> | null = null

  // Queue bounce animation
  let queueBounce = $state(false)

  const navItems = [
    { href: "/tools/ytdlp/settings", icon: "settings" },
  ]

  function isActive(href: string): boolean {
    const path = $page.url.pathname
    if (href === "/tools/ytdlp") return path === "/tools/ytdlp"
    return path.startsWith(href)
  }

  // Popup auto-refresh
  let popupInterval: ReturnType<typeof setInterval> | null = null
  let unlisten: (() => void) | null = null

  async function loadActiveDownloads() {
    try {
      const result = await commands.getActiveDownloads()
      if (result.status === "ok") {
        activeDownloads = result.data
      }
    } catch (e) { console.error("Failed to load active downloads:", e) }
  }

  async function handleCancelAll() {
    try {
      const result = await commands.cancelAllDownloads()
      if (result.status === "ok") {
        await loadActiveDownloads()
      }
    } catch (e) { console.error("Failed to cancel all downloads:", e) }
  }

  async function loadRecentCompleted() {
    try {
      const result = await commands.getDownloadQueue()
      if (result.status === "ok") {
        recentCompleted = result.data.filter((d: any) => d.status === "completed").slice(0, 5)
      }
    } catch (e) { console.error("Failed to load recent completed:", e) }
  }

  function startPopupRefresh() {
    loadActiveDownloads()
    loadRecentCompleted()
    popupInterval = setInterval(() => {
      loadActiveDownloads()
      loadRecentCompleted()
    }, 2000)
  }

  function stopPopupRefresh() {
    if (popupInterval) {
      clearInterval(popupInterval)
      popupInterval = null
    }
  }

  // 3-1: Return cleanup function to prevent interval leak
  $effect(() => {
    if (popupOpen) {
      startPopupRefresh()
    } else {
      stopPopupRefresh()
    }
    return () => { stopPopupRefresh() }
  })

  function showToast(message: string, icon = "check_circle") {
    toastMessage = message
    toastIcon = icon
    toastVisible = true
    if (toastTimeout) clearTimeout(toastTimeout)
    toastTimeout = setTimeout(() => { toastVisible = false }, 3000)
  }

  function handleQueueAdded(e: Event) {
    const count = (e as CustomEvent).detail?.count ?? 1
    showToast(t("layout.queueAdded", { count }))

    queueBounce = false
    requestAnimationFrame(() => { queueBounce = true })
    setTimeout(() => { queueBounce = false }, 800)

    loadActiveDownloads()
  }

  onMount(async () => {
    // Detect OS platform
    try {
      const p = platform()
      if (p === "windows") currentPlatform = "windows"
      else if (p === "linux") currentPlatform = "linux"
      else currentPlatform = "macos"
    } catch { currentPlatform = "macos" }

    await checkDeps()

    // Initialize i18n and theme from saved settings
    try {
      const settingsResult = await commands.getSettings()
      if (settingsResult.status === "ok") {
        await initLocale(settingsResult.data.language)
        initTheme(settingsResult.data.theme)
      } else {
        await initLocale()
        initTheme()
      }
    } catch (e) {
      await initLocale()
      initTheme()
    }

    try {
      const unlistenFn = await listen("download-event", (event: any) => {
        const data = event.payload
        if (data.eventType === "completed") {
          const title = activeDownloads.find(d => d.id === data.taskId)?.title
          showToast(t("layout.downloadComplete", { title: title || "video" }), "download_done")
        }
        loadActiveDownloads()
      })
      unlisten = unlistenFn
    } catch (e) { console.error("Failed to listen for download events:", e) }

    loadActiveDownloads()

    window.addEventListener("queue-added", handleQueueAdded)
  })

  onDestroy(() => {
    stopPopupRefresh()
    if (unlisten) unlisten()
    window.removeEventListener("queue-added", handleQueueAdded)
    if (toastTimeout) clearTimeout(toastTimeout)
  })

  function handleDebugKey(e: KeyboardEvent) {
    if (e.key === "F10") {
      e.preventDefault()
      showDebug = !showDebug
      if (showDebug) {
        // Refresh debug info after overlay is shown
        commands.checkDependencies().then(result => {
          if (result.status === "ok") {
            ytdlpDebug = result.data.ytdlpDebug ?? ""
          }
        }).catch(() => {})
      }
    }
  }

  async function checkDeps() {
    checking = true
    try {
      const result = await commands.checkDependencies()
      if (result.status === "ok") {
        ytdlpInstalled = result.data.ytdlpInstalled
        ytdlpVersion = result.data.ytdlpVersion ?? null
        ffmpegInstalled = result.data.ffmpegInstalled
        ffmpegVersion = result.data.ffmpegVersion ?? null
        depsInstalled = result.data.ytdlpInstalled
        ytdlpDebug = result.data.ytdlpDebug ?? ""
      }
    } catch (e) {
      console.error(e)
    } finally {
      checking = false
    }
  }

  type InstallInfo = {
    recommended: string
    alternative: string
  }

  const installCommands: Record<string, InstallInfo> = {
    macos: {
      recommended: "brew install yt-dlp ffmpeg",
      alternative: "pip install yt-dlp",
    },
    windows: {
      recommended: "winget install yt-dlp.yt-dlp && winget install Gyan.FFmpeg",
      alternative: "scoop install yt-dlp ffmpeg",
    },
    linux: {
      recommended: "sudo apt install yt-dlp ffmpeg",
      alternative: "pip install yt-dlp",
    },
  }

  let platformCommands = $derived(installCommands[currentPlatform] || installCommands.macos)

  async function copyCommand(cmd: string) {
    await navigator.clipboard.writeText(cmd)
    copiedCmd = cmd
    setTimeout(() => { copiedCmd = null }, 2000)
  }
</script>

<svelte:window onkeydown={handleDebugKey} />

<div class="flex flex-col h-screen overflow-hidden bg-yt-bg">
  <!-- Top Header Bar -->
  <header class="h-12 bg-yt-surface border-b border-white/[0.06] flex items-center justify-between px-5 shrink-0 z-30">
    <!-- Left: Logo -->
    <a href="/tools/ytdlp" class="flex items-center gap-3 hover:opacity-80 transition-opacity">
      <div class="w-7 h-7 rounded-lg bg-yt-primary flex items-center justify-center text-white shrink-0">
        <span class="material-symbols-outlined text-[20px]">download</span>
      </div>
      <h1 class="font-display font-bold text-base text-gray-100">Modern YT-DLP GUI</h1>
    </a>

    <!-- Right: Actions -->
    <div class="flex items-center gap-1">
      <!-- Queue Popup Toggle -->
      <div class="relative">
        <button
          onclick={() => popupOpen = !popupOpen}
          class="flex items-center gap-2 px-3 py-2 rounded-lg text-gray-400 hover:bg-white/[0.06] hover:text-gray-100 transition-colors relative {queueBounce ? 'animate-queue-bounce' : ''}"
          title={t("nav.activeDownloads")}
        >
          <span class="material-symbols-outlined text-[20px]">downloading</span>
          <span class="text-sm hidden sm:inline">{t("nav.queue")}</span>
          {#if (activeCount + pendingCount) > 0}
            <span class="absolute top-1 right-1 w-5 h-5 bg-yt-primary text-white text-[10px] font-bold rounded-full flex items-center justify-center">
              {activeCount + pendingCount}
            </span>
          {/if}
        </button>
      </div>

      <div class="h-6 w-px bg-white/[0.06] mx-1"></div>

      {#each navItems as item}
        <a
          href={item.href}
          class="flex items-center gap-2 px-3 py-2 rounded-lg transition-colors text-sm
            {isActive(item.href) ? 'bg-yt-primary/10 text-yt-primary font-medium' : 'text-gray-400 hover:bg-white/[0.06] hover:text-gray-100'}"
        >
          <span class="material-symbols-outlined text-[20px]">{item.icon}</span>
          <span class="hidden sm:inline">{t("nav.settings")}</span>
        </a>
      {/each}
    </div>
  </header>

  <!-- Main Content -->
  <main class="flex-1 flex overflow-hidden relative">
    <div class="absolute top-0 left-0 w-full h-[500px] bg-gradient-to-b from-yt-primary/[0.03] to-transparent pointer-events-none z-0"></div>

    {#if checking}
      <div class="flex-1 flex items-center justify-center z-10">
        <div class="flex flex-col items-center gap-3">
          <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
          <p class="text-sm text-gray-400">{t("layout.checkingDeps")}</p>
        </div>
      </div>
    {:else if !depsInstalled}
      <div class="flex-1 flex flex-col items-center justify-center z-10 gap-5 px-4">
        <div class="w-16 h-16 rounded-xl bg-yt-primary/20 flex items-center justify-center">
          <span class="material-symbols-outlined text-yt-primary text-4xl">download</span>
        </div>
        <h2 class="font-display text-2xl font-bold text-gray-100">{t("layout.setupRequired")}</h2>
        <p class="text-gray-400 text-center max-w-md">{t("layout.setupDesc")}</p>

        <!-- Dependency Status -->
        <div class="w-full max-w-md flex gap-3">
          <div class="flex-1 flex items-center gap-3 bg-yt-highlight border border-white/[0.06] rounded-lg px-4 py-3">
            <span class="material-symbols-outlined text-[20px] {ytdlpInstalled ? 'text-green-400' : 'text-red-400'}">
              {ytdlpInstalled ? "check_circle" : "cancel"}
            </span>
            <div class="min-w-0">
              <p class="text-sm font-semibold text-gray-100">yt-dlp</p>
              <p class="text-xs truncate {ytdlpInstalled ? 'text-green-400/80' : 'text-red-400/80'}">
                {ytdlpInstalled ? ytdlpVersion ?? t("layout.installed") : t("layout.notInstalled")}
              </p>
            </div>
          </div>
          <div class="flex-1 flex items-center gap-3 bg-yt-highlight border border-white/[0.06] rounded-lg px-4 py-3">
            <span class="material-symbols-outlined text-[20px] {ffmpegInstalled ? 'text-green-400' : 'text-red-400'}">
              {ffmpegInstalled ? "check_circle" : "cancel"}
            </span>
            <div class="min-w-0">
              <p class="text-sm font-semibold text-gray-100">ffmpeg</p>
              <p class="text-xs truncate {ffmpegInstalled ? 'text-green-400/80' : 'text-red-400/80'}">
                {ffmpegInstalled ? t("layout.installed") : t("layout.notInstalled")}
              </p>
            </div>
          </div>
        </div>

        <p class="text-gray-500 text-sm">{t("layout.installGuide")}</p>

        <!-- Recommended command -->
        <div class="w-full max-w-md">
          <div class="text-xs text-yt-primary font-semibold mb-1.5 uppercase tracking-wider">{t("layout.recommended")}</div>
          <div class="flex items-center gap-2 bg-yt-highlight border border-white/[0.06] rounded-lg px-4 py-3">
            <code class="flex-1 text-sm text-gray-200 font-mono select-all">{platformCommands.recommended}</code>
            <button
              onclick={() => copyCommand(platformCommands.recommended)}
              class="shrink-0 p-1.5 rounded-md hover:bg-white/[0.08] transition-colors"
              title="Copy"
            >
              <span class="material-symbols-outlined text-[18px] {copiedCmd === platformCommands.recommended ? 'text-green-400' : 'text-gray-400'}">
                {copiedCmd === platformCommands.recommended ? "check" : "content_copy"}
              </span>
            </button>
          </div>
        </div>

        <!-- Alternative -->
        <div class="text-xs text-gray-500">{t("layout.altMethod")}</div>

        <div class="w-full max-w-md">
          <div class="flex items-center gap-2 bg-yt-highlight/50 border border-white/[0.04] rounded-lg px-4 py-3">
            <code class="flex-1 text-sm text-gray-400 font-mono select-all">{platformCommands.alternative}</code>
            <button
              onclick={() => copyCommand(platformCommands.alternative)}
              class="shrink-0 p-1.5 rounded-md hover:bg-white/[0.08] transition-colors"
              title="Copy"
            >
              <span class="material-symbols-outlined text-[18px] {copiedCmd === platformCommands.alternative ? 'text-green-400' : 'text-gray-400'}">
                {copiedCmd === platformCommands.alternative ? "check" : "content_copy"}
              </span>
            </button>
          </div>
        </div>

        <!-- ffmpeg note -->
        <p class="text-xs text-gray-500 flex items-center gap-1.5 max-w-md">
          <span class="material-symbols-outlined text-[16px]">info</span>
          {t("layout.ffmpegNote")}
        </p>

        <!-- Recheck button -->
        <button
          class="px-8 py-3 rounded-xl bg-yt-primary hover:bg-blue-500 text-white font-bold transition-all shadow-lg shadow-yt-primary/20 mt-2"
          onclick={checkDeps}
        >
          {t("layout.recheck")}
        </button>
      </div>
    {:else}
      <div class="flex-1 z-10 overflow-hidden">
        {@render children()}
      </div>
    {/if}
  </main>

  <!-- Download Popup -->
  {#if popupOpen}
    <!-- Backdrop -->
    <button
      class="fixed inset-0 bg-black/50 z-40"
      onclick={() => popupOpen = false}
      aria-label="Close popup"
    ></button>

    <!-- Floating Popup -->
    <div class="fixed top-12 right-4 w-96 max-h-[70vh] bg-yt-surface rounded-xl shadow-2xl shadow-black/40 z-50 flex flex-col border border-white/[0.06] animate-popup-in">
      <!-- Header -->
      <div class="px-4 py-3 border-b border-white/[0.06] flex items-center justify-between shrink-0">
        <h3 class="font-display font-semibold text-sm text-gray-100">{t("nav.queue")}</h3>
        <div class="flex items-center gap-1">
          {#if (activeCount + pendingCount) > 0}
            <button
              onclick={handleCancelAll}
              class="text-amber-400 hover:bg-amber-500/10 text-xs font-medium px-2 py-1 rounded-lg transition-colors"
            >
              {t("layout.cancelAll")}
            </button>
          {/if}
          <button onclick={() => popupOpen = false} class="text-gray-500 hover:text-gray-400 transition-colors p-1 rounded-lg hover:bg-white/[0.06]">
            <span class="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>
      </div>

      <!-- Active Downloads -->
      <div class="flex-1 overflow-y-auto hide-scrollbar">
        {#if activeDownloads.length === 0}
          <div class="flex flex-col items-center justify-center py-12 text-center px-4">
            <span class="material-symbols-outlined text-gray-600 text-4xl">cloud_done</span>
            <p class="text-gray-400 text-sm mt-2">{t("layout.noActiveDownloads")}</p>
          </div>
        {:else}
          <div class="p-3 space-y-2">
            {#each activeDownloads as item}
              <div class="bg-yt-highlight rounded-lg p-3 border border-white/[0.06] {item.status === 'downloading' ? '!border-yt-primary/30' : ''}">
                <p class="text-sm text-gray-100 truncate font-medium">{item.title}</p>
                <div class="flex items-center justify-between mt-1.5">
                  {#if item.status === "downloading"}
                    <span class="text-xs text-yt-primary font-mono">{(item.progress || 0).toFixed(0)}%</span>
                    <span class="text-xs text-gray-400">{item.speed || "..."}</span>
                  {:else}
                    <span class="inline-flex items-center gap-1 text-xs text-amber-400 bg-amber-500/10 px-2 py-0.5 rounded-full">
                      <span class="material-symbols-outlined text-[14px]">schedule</span>
                      {t("layout.queued")}
                    </span>
                  {/if}
                </div>
                {#if item.status === "downloading"}
                  <div class="w-full bg-white/[0.06] rounded-full h-1 mt-2">
                    <div class="bg-yt-primary h-1 rounded-full transition-all" style="width: {item.progress || 0}%"></div>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <!-- Recent Completed -->
        {#if recentCompleted.length > 0}
          <div class="border-t border-white/[0.06] mt-2">
            <div class="px-4 py-2">
              <h4 class="text-xs text-gray-400 font-medium uppercase tracking-wider">{t("layout.recent")}</h4>
            </div>
            <div class="px-3 pb-3 space-y-1">
              {#each recentCompleted as item}
                <div class="flex items-center gap-2 px-2 py-1.5 rounded-lg">
                  <span class="material-symbols-outlined text-green-600 text-[16px]">check_circle</span>
                  <span class="text-sm text-gray-400 truncate">{item.title}</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <!-- Footer: View All -->
      <div class="border-t border-white/[0.06] px-4 py-2.5 shrink-0">
        <a href="/tools/ytdlp/queue" class="flex items-center justify-center gap-1.5 text-sm text-yt-primary hover:text-blue-400 font-medium transition-colors" onclick={() => popupOpen = false}>
          <span>{t("layout.viewAll")}</span>
          <span class="material-symbols-outlined text-[16px]">arrow_forward</span>
        </a>
      </div>
    </div>
  {/if}

  <!-- Toast Notification -->
  {#if toastVisible}
    <div class="fixed bottom-6 right-6 z-[60] animate-toast-in">
      <div class="flex items-center gap-3 bg-white/10 backdrop-blur-xl text-white px-5 py-3 rounded-xl shadow-2xl">
        <span class="material-symbols-outlined text-[20px] {toastIcon === 'download_done' ? 'text-green-400' : 'text-yt-primary'}">{toastIcon}</span>
        <span class="text-sm font-medium">{toastMessage}</span>
      </div>
    </div>
  {/if}

  <!-- Debug Overlay (F10) -->
  {#if showDebug}
    <button
      class="fixed inset-0 bg-black/70 z-[100]"
      onclick={() => showDebug = false}
      aria-label="Close debug"
    ></button>
    <div class="fixed inset-0 z-[101] flex items-center justify-center pointer-events-none">
      <div class="bg-[#1a1a2e] border border-white/10 rounded-2xl shadow-2xl p-6 max-w-lg w-full mx-4 pointer-events-auto">
        <div class="flex items-center justify-between mb-4">
          <h3 class="text-sm font-bold text-gray-200 flex items-center gap-2">
            <span class="material-symbols-outlined text-[18px] text-amber-400">bug_report</span>
            Debug Info
          </h3>
          <button onclick={() => showDebug = false} class="text-gray-500 hover:text-gray-300 transition-colors">
            <span class="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>
        <div class="space-y-3">
          <div>
            <p class="text-xs text-gray-400 mb-1">yt-dlp status</p>
            <p class="text-sm {depsInstalled ? 'text-green-400' : 'text-red-400'}">
              {depsInstalled ? "Installed" : "Not detected"}
            </p>
          </div>
          {#if ytdlpDebug}
            <div>
              <p class="text-xs text-gray-400 mb-1">Detection log</p>
              <pre class="text-xs text-gray-300 bg-black/40 rounded-lg p-3 whitespace-pre-wrap break-all font-mono">{ytdlpDebug}</pre>
            </div>
          {:else if depsInstalled}
            <p class="text-xs text-gray-500">No issues detected.</p>
          {/if}
        </div>
        <p class="text-[10px] text-gray-600 mt-4 text-right">Press F10 to close</p>
      </div>
    </div>
  {/if}
</div>

<style>
  @keyframes popup-in {
    from { opacity: 0; transform: translateY(-8px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .animate-popup-in {
    animation: popup-in 0.15s ease-out;
  }

  @keyframes toast-in {
    from { opacity: 0; transform: translateY(12px) scale(0.95); }
    to { opacity: 1; transform: translateY(0) scale(1); }
  }
  .animate-toast-in {
    animation: toast-in 0.25s ease-out;
  }

  @keyframes queue-bounce {
    0%, 100% { transform: scale(1); }
    25% { transform: scale(1.2); }
    50% { transform: scale(0.95); }
    75% { transform: scale(1.1); }
  }
  :global(.animate-queue-bounce) {
    animation: queue-bounce 0.6s ease-in-out;
  }
</style>
