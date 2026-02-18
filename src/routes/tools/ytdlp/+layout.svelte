<script lang="ts">
  import { commands } from "$lib/bindings"
  import { invoke } from "@tauri-apps/api/core"
  import { listen } from "@tauri-apps/api/event"
  import { platform } from "@tauri-apps/plugin-os"
  import { getVersion } from "@tauri-apps/api/app"
  import { page } from "$app/stores"
  import { onMount, onDestroy } from "svelte"
  import { openUrl } from "@tauri-apps/plugin-opener"
  import { t, initLocale } from "$lib/i18n/index.svelte"
  import { initTheme } from "$lib/theme/index.svelte"
  import { check, type Update } from "@tauri-apps/plugin-updater"
  import { relaunch } from "@tauri-apps/plugin-process"
  import type { ActiveDownload, ProgressCacheEntry } from "$lib/types"

  let { children } = $props()

  import type { FullDependencyStatus, DepInstallEvent } from "$lib/bindings"

  let checking = $state(true)
  let ytdlpInstalled = $state(false)
  let ytdlpVersion = $state<string | null>(null)
  let ffmpegInstalled = $state(false)
  let ffmpegVersion = $state<string | null>(null)
  let ytdlpDebug = $state("")
  let recentLogs = $state("")
  let showDebug = $state(false)
  let logsCopied = $state(false)
  let currentPlatform = $state<string>("macos")
  let copiedCmd = $state<string | null>(null)
  let appVersion = $state("...")

  // Auto-install state
  let fullDepStatus = $state<FullDependencyStatus | null>(null)
  let depsInstalled = $derived(
    fullDepStatus
      ? fullDepStatus.ytdlp.installed && fullDepStatus.ffmpeg.installed && fullDepStatus.deno.installed
      : false
  )
  let installing = $state(false)
  let installProgress = $state<Record<string, { stage: string, percent: number, message: string | null }>>({})
  let installError = $state<string | null>(null)
  let showManualInstall = $state(false)
  let unlistenDepInstall: (() => void) | null = null

  // Popup state
  let popupOpen = $state(false)
  let activeDownloads = $state<ActiveDownload[]>([])
  let recentCompleted = $state<ActiveDownload[]>([])
  let progressCache = $state<Map<number, ProgressCacheEntry>>(new Map())
  let activeCount = $derived(activeDownloads.filter(d => d.status === "downloading").length)
  let pendingCount = $derived(activeDownloads.filter(d => d.status === "pending").length)

  // Toast state
  let toastMessage = $state("")
  let toastVisible = $state(false)
  let toastIcon = $state("check_circle")
  let toastType = $state<"success" | "error">("success")
  let toastTimeout: ReturnType<typeof setTimeout> | null = null

  // Update state
  let updateAvailable = $state(false)
  let updateInfo = $state<Update | null>(null)
  let showUpdateDialog = $state(false)
  let updateDownloading = $state(false)
  let updateProgress = $state(0)
  let updateTotalSize = $state(0)
  let updateDownloaded = $state(0)
  let updateReady = $state(false)

  // Welcome (first-run) state
  let setupCompleted = $state<boolean | null>(null) // null = loading
  let selectedDepMode = $state<"external" | "system" | null>(null)
  let currentDepMode = $state<string>("external")

  // Debug command menu state (F9)
  let showDebugCmd = $state(false)
  let resetConfirming = $state(false)
  let resetting = $state(false)
  let debugCmdResults = $state<Record<string, { status: "idle" | "loading" | "success" | "error", message: string }>>({
    "yt-dlp": { status: "idle", message: "" },
    "ffmpeg": { status: "idle", message: "" },
    "deno": { status: "idle", message: "" },
  })
  let debugDepStatus = $state<FullDependencyStatus | null>(null)
  let debugDepLoading = $state(false)

  async function debugRefreshStatus() {
    debugDepLoading = true
    try {
      const r = await commands.checkFullDependencies(true)
      if (r.status === "ok") debugDepStatus = r.data
    } catch (e) { console.error(e) }
    debugDepLoading = false
  }

  async function debugDeleteDep(depName: string) {
    debugCmdResults[depName] = { status: "loading", message: "" }
    debugCmdResults = { ...debugCmdResults }
    try {
      const r = await commands.deleteAppManagedDep(depName)
      if (r.status === "ok") {
        debugCmdResults[depName] = { status: "success", message: r.data }
      } else {
        debugCmdResults[depName] = { status: "error", message: Object.values(r.error)[0] as string }
      }
    } catch (e: any) {
      debugCmdResults[depName] = { status: "error", message: e?.message || String(e) }
    }
    debugCmdResults = { ...debugCmdResults }
    await debugRefreshStatus()
  }

  // Close dialog state
  let showCloseDialog = $state(false)
  let rememberChoice = $state(false)
  let unlistenClose: (() => void) | null = null

  // Queue flash animation
  let queueFlash = $state(false)

  const navItems = [
    { href: "/tools/ytdlp", icon: "download", labelKey: "nav.downloader", exact: true },
    { href: "/tools/ytdlp/queue", icon: "toc", labelKey: "nav.queueHistory" }, // explicit queue page link
    { href: "/tools/ytdlp/logs", icon: "terminal", labelKey: "nav.logs" },
    { href: "/tools/ytdlp/settings", icon: "settings", labelKey: "nav.settings" },
  ]

  function isActive(href: string, exact = false): boolean {
    const path = $page.url.pathname
    if (exact) return path === href
    return path.startsWith(href)
  }

  // Popup auto-refresh
  let popupInterval: ReturnType<typeof setInterval> | null = null
  let unlisten: (() => void) | null = null
  let loadDebounceTimer: ReturnType<typeof setTimeout> | null = null

  async function loadQueueSummary() {
    try {
      const result = await commands.getQueueSummary()
      if (result.status === "ok") {
        const data = result.data
        activeDownloads = data.activeItems.map((item: any) => {
          const cached = progressCache.get(item.id)
          if (cached && item.status === "downloading") {
            return { ...item, ...cached }
          }
          return item
        })
        recentCompleted = data.recentCompleted
        // 더 이상 활성이 아닌 다운로드의 캐시 정리
        const activeIds = new Set(data.activeItems.map((d: any) => d.id))
        for (const id of progressCache.keys()) {
          if (!activeIds.has(id)) progressCache.delete(id)
        }
      }
    } catch (e) { console.error("Failed to load queue summary:", e) }
  }

  function debouncedLoadQueueSummary() {
    if (loadDebounceTimer) clearTimeout(loadDebounceTimer)
    loadDebounceTimer = setTimeout(() => {
      loadQueueSummary()
    }, 150)
  }

  async function handleCancelAll() {
    try {
      const result = await commands.cancelAllDownloads()
      if (result.status === "ok") {
        await queueSummaryPromise
      }
    } catch (e) { console.error("Failed to cancel all downloads:", e) }
    // Refresh immediately
    loadQueueSummary()
  }

  const queueSummaryPromise = loadQueueSummary()

  function startPopupRefresh() {
    loadQueueSummary()
    popupInterval = setInterval(() => {
      loadQueueSummary()
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

  function showToast(message: string, icon = "check_circle", type: "success" | "error" = "success") {
    toastMessage = message
    toastIcon = icon
    toastType = type
    toastVisible = true
    if (toastTimeout) clearTimeout(toastTimeout)
    toastTimeout = setTimeout(() => { toastVisible = false }, 3000)
  }

  function showErrorToast(message: string) {
    showToast(message, "error", "error")
  }

  function handleQueueAdded(e: Event) {
    const count = (e as CustomEvent).detail?.count ?? 1
    showToast(t("layout.queueAdded", { count }))

    queueFlash = false
    requestAnimationFrame(() => { queueFlash = true })
    setTimeout(() => { queueFlash = false }, 400)

    debouncedLoadQueueSummary()
  }

  onMount(async () => {
    // Detect OS platform
    try {
      const p = platform()
      if (p === "windows") currentPlatform = "windows"
      else if (p === "linux") currentPlatform = "linux"
      else currentPlatform = "macos"
    } catch { currentPlatform = "macos" }

    try { appVersion = await getVersion() } catch { appVersion = "0.0.0" }

    // Check for updates in background
    checkForUpdate()

    // Try to load cached dep status for instant UI (no spinner)
    try {
      const cachedResult = await commands.getCachedDepStatus()
      if (cachedResult.status === "ok" && cachedResult.data) {
        const cached = cachedResult.data
        fullDepStatus = cached
        ytdlpInstalled = cached.ytdlp.installed
        ytdlpVersion = cached.ytdlp.version ?? null
        ffmpegInstalled = cached.ffmpeg.installed
        ffmpegVersion = cached.ffmpeg.version ?? null
        // If all deps were installed in cache, skip spinner and show main UI immediately
        if (cached.ytdlp.installed && cached.ffmpeg.installed && cached.deno.installed) {
          checking = false
        }
      }
    } catch {
      // No cache available, will fall through to normal check with spinner
    }

    // Run full check: background if cache hit (no spinner), foreground if no cache
    const hasCachedDeps = !checking
    if (hasCachedDeps) {
      checkDeps(false, true) // background: don't set checking=true
    } else {
      checkDeps() // foreground: show spinner until done
    }

    // Initialize i18n, theme, and check setup status from saved settings
    try {
      const settingsResult = await commands.getSettings()
      if (settingsResult.status === "ok") {
        await initLocale(settingsResult.data.language)
        initTheme(settingsResult.data.theme)
        setupCompleted = settingsResult.data.setupCompleted
        currentDepMode = settingsResult.data.depMode
      } else {
        await initLocale()
        initTheme()
        setupCompleted = false
      }
    } catch (e) {
      await initLocale()
      initTheme()
      setupCompleted = false
    }

    try {
      const unlistenFn = await listen("download-event", (event: any) => {
        const data = event.payload

        if (data.eventType === "progress") {
          const cached = {
            progress: data.percent ?? 0,
            speed: data.speed ?? null,
            eta: data.eta ?? null,
          }
          progressCache.set(data.taskId, cached)
          const idx = activeDownloads.findIndex(d => d.id === data.taskId)
          if (idx !== -1) {
            activeDownloads[idx] = { ...activeDownloads[idx], ...cached }
          }
        } else {
          // 상태 변경 이벤트(started, completed, error, cancelled)만 DB 재조회
          if (data.eventType === "completed") {
            const title = activeDownloads.find(d => d.id === data.taskId)?.title
            showToast(t("layout.downloadComplete", { title: title || "video" }), "download_done")
          }
          debouncedLoadQueueSummary()
        }
      })
      unlisten = unlistenFn
    } catch (e) { console.error("Failed to listen for download events:", e) }

    loadQueueSummary()

    window.addEventListener("queue-added", handleQueueAdded)

    // Listen for close-requested event from backend
    try {
      const unlistenCloseFn = await listen("close-requested", () => {
        showCloseDialog = true
      })
      unlistenClose = unlistenCloseFn
    } catch (e) { console.error("Failed to listen for close-requested:", e) }
  })

  onDestroy(() => {
    stopPopupRefresh()
    if (unlisten) unlisten()
    if (unlistenClose) unlistenClose()
    if (unlistenDepInstall) unlistenDepInstall()
    window.removeEventListener("queue-added", handleQueueAdded)
    if (toastTimeout) clearTimeout(toastTimeout)
    if (loadDebounceTimer) clearTimeout(loadDebounceTimer)
  })

  function handleDebugKey(e: KeyboardEvent) {
    if (e.key === "F10") {
      e.preventDefault()
      showDebug = !showDebug
      if (showDebug) {
        logsCopied = false
        commands.checkDependencies().then(result => {
          if (result.status === "ok") {
            ytdlpDebug = result.data.ytdlpDebug ?? ""
          }
        }).catch(() => {})
        invoke<string>("get_recent_logs").then(data => {
          recentLogs = data
        }).catch(() => {})
      }
    }
    if (e.key === "F9") {
      e.preventDefault()
      showDebugCmd = !showDebugCmd
      if (showDebugCmd) {
        debugCmdResults = {
          "yt-dlp": { status: "idle", message: "" },
          "ffmpeg": { status: "idle", message: "" },
          "deno": { status: "idle", message: "" },
        }
        debugRefreshStatus()
      }
    }
  }

  async function copyLogs() {
    const text = recentLogs || ytdlpDebug || "No logs available"
    await navigator.clipboard.writeText(text)
    logsCopied = true
    setTimeout(() => { logsCopied = false }, 2000)
  }

  async function handleCloseChoice(minimize: boolean) {
    try {
      await commands.setMinimizeToTray(minimize, rememberChoice)
    } catch (e) { console.error("Failed to set minimize to tray:", e) }
    showCloseDialog = false
    rememberChoice = false
  }

  async function checkDeps(force = false, background = false) {
    // In background mode, don't show spinner (cached UI is already visible)
    if (!background) {
      checking = true
    }
    try {
      const fullResult = await commands.checkFullDependencies(force)
      if (fullResult.status === "ok") {
        fullDepStatus = fullResult.data
        ytdlpInstalled = fullResult.data.ytdlp.installed
        ytdlpVersion = fullResult.data.ytdlp.version ?? null
        ffmpegInstalled = fullResult.data.ffmpeg.installed
        ffmpegVersion = fullResult.data.ffmpeg.version ?? null
        // Build debug info
        ytdlpDebug = `yt-dlp: ${fullResult.data.ytdlp.installed ? fullResult.data.ytdlp.version : "not found"} (${fullResult.data.ytdlp.source})\nffmpeg: ${fullResult.data.ffmpeg.installed ? fullResult.data.ffmpeg.version : "not found"} (${fullResult.data.ffmpeg.source})\ndeno: ${fullResult.data.deno.installed ? fullResult.data.deno.version : "not found"} (${fullResult.data.deno.source})`
      }
    } catch (e) {
      console.error(e)
      // Fallback to legacy check
      try {
        const result = await commands.checkDependencies()
        if (result.status === "ok") {
          ytdlpInstalled = result.data.ytdlpInstalled
          ytdlpVersion = result.data.ytdlpVersion ?? null
          ffmpegInstalled = result.data.ffmpegInstalled
          ffmpegVersion = result.data.ffmpegVersion ?? null
          ytdlpDebug = result.data.ytdlpDebug ?? ""
        }
      } catch (e2) {
        console.error(e2)
      }
    } finally {
      checking = false
    }
  }

  async function handleAutoInstall() {
    installing = true
    installError = null
    installProgress = {}

    // Listen for install progress events
    try {
      const unlistenFn = await listen("dep-install-event", (event: any) => {
        const data = event.payload as DepInstallEvent
        installProgress[data.depName] = {
          stage: data.stage,
          percent: data.percent,
          message: data.message ?? null,
        }
        installProgress = { ...installProgress }

        // Immediately mark dep as installed when Completing stage is received
        if (data.stage === "Completing" && fullDepStatus) {
          const depKey = data.depName === "yt-dlp" ? "ytdlp" : data.depName as "ffmpeg" | "deno"
          fullDepStatus = {
            ...fullDepStatus,
            [depKey]: {
              ...fullDepStatus[depKey],
              installed: true,
              source: "AppManaged",
            },
          }
        }
      })
      unlistenDepInstall = unlistenFn
    } catch (e) {
      console.error("Failed to listen for dep install events:", e)
    }

    try {
      const result = await commands.installAllDependencies()
      if (result.status === "ok") {
        // Check if any failed
        const failures = result.data.filter(r => r.includes("FAILED"))
        if (failures.length > 0) {
          installError = failures.join("\n")
        }
      } else {
        installError = Object.values(result.error)[0] as string
      }
    } catch (e: any) {
      installError = e?.message || String(e)
    } finally {
      installing = false
      if (unlistenDepInstall) {
        unlistenDepInstall()
        unlistenDepInstall = null
      }
      // Recheck dependencies (force refresh after install)
      await checkDeps(true)
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
      recommended: "winget install yt-dlp; winget install Gyan.FFmpeg",
      alternative: "scoop install yt-dlp ffmpeg",
    },
    linux: {
      recommended: "sudo apt install yt-dlp ffmpeg",
      alternative: "pip install yt-dlp",
    },
  }

  async function checkForUpdate() {
    try {
      const update = await check()
      if (update) {
        updateAvailable = true
        updateInfo = update
      }
    } catch {
      // Silently fail in dev mode or when updater is not configured
    }
  }

  async function handleUpdate() {
    if (!updateInfo) return
    updateDownloading = true
    updateDownloaded = 0
    updateTotalSize = 0
    updateProgress = 0
    try {
      await updateInfo.downloadAndInstall((progress) => {
        if (progress.event === "Started" && progress.data.contentLength) {
          updateTotalSize = progress.data.contentLength
        } else if (progress.event === "Progress") {
          updateDownloaded += progress.data.chunkLength
          if (updateTotalSize > 0) {
            updateProgress = Math.round((updateDownloaded / updateTotalSize) * 100)
          }
        } else if (progress.event === "Finished") {
          updateReady = true
        }
      })
      await relaunch()
    } catch (e) {
      console.error("Update failed:", e)
      updateDownloading = false
    }
  }

  async function handleWelcomeComplete() {
    if (!selectedDepMode) return
    try {
      const settingsResult = await commands.getSettings()
      if (settingsResult.status === "ok") {
        const updated = { ...settingsResult.data, depMode: selectedDepMode, setupCompleted: true }
        await commands.updateSettings(updated)
      }
    } catch (e) {
      console.error("Failed to save welcome settings:", e)
    }
    setupCompleted = true
    // Trigger dep check after mode selection
    await checkDeps(true)
  }

  async function handleSwitchToAppManaged() {
    try {
      const settingsResult = await commands.getSettings()
      if (settingsResult.status === "ok") {
        const updated = { ...settingsResult.data, depMode: "external" as const }
        await commands.updateSettings(updated)
        currentDepMode = "external"
      }
    } catch (e) {
      console.error("Failed to switch dep mode:", e)
    }
    await handleAutoInstall()
  }

  async function handleFactoryReset() {
    resetting = true
    try {
      await commands.resetAllData()
    } catch (e) {
      console.error("Reset failed:", e)
    }
    // Relaunch the app
    try {
      await relaunch()
    } catch {
      // If relaunch fails, just reload the page
      window.location.reload()
    }
  }

  let platformCommands = $derived(installCommands[currentPlatform] || installCommands.macos)

  let copiedCmdTimeout: ReturnType<typeof setTimeout> | null = null
  async function copyCommand(cmd: string) {
    await navigator.clipboard.writeText(cmd)
    copiedCmd = cmd
    if (copiedCmdTimeout) clearTimeout(copiedCmdTimeout)
    copiedCmdTimeout = setTimeout(() => { copiedCmd = null }, 2000)
  }
</script>

<svelte:window onkeydown={handleDebugKey} />

<div class="flex h-screen overflow-hidden bg-yt-bg text-yt-text font-body selection:bg-yt-primary/20 selection:text-yt-text">
  <!-- Sidebar -->
  <aside class="w-56 bg-yt-surface border-r border-yt-border flex flex-col shrink-0 z-20">
    <!-- Window Drag Region (Mac style) -->
    <div data-tauri-drag-region class="h-8 shrink-0"></div>

    <!-- App Title/Logo -->
    <div class="px-5 pb-6 pt-2">
       <div class="flex items-center gap-3">
        <div class="w-8 h-8 rounded-lg bg-yt-primary flex items-center justify-center text-white shrink-0 shadow-lg shadow-yt-primary/30">
          <span class="material-symbols-outlined text-[20px]">download</span>
        </div>
        <div>
          <h1 class="font-display font-semibold text-sm text-yt-text tracking-tight">Modern YT-DLP</h1>
          <p class="text-[10px] text-yt-text-secondary font-mono">v{appVersion}</p>
        </div>
       </div>
    </div>

    <!-- Navigation -->
    <nav class="flex-1 px-3 space-y-1 overflow-y-auto">
      {#each navItems as item}
        <a
          href={item.href}
          class="flex items-center gap-3 px-3 py-2 rounded-md transition-colors text-sm font-medium relative
            {isActive(item.href, item.exact) 
              ? 'bg-yt-highlight text-yt-text shadow-sm ring-1 ring-inset ring-yt-border' 
              : 'text-yt-text-secondary hover:bg-yt-overlay hover:text-yt-text'}"
        >
          <span class="material-symbols-outlined text-[20px] {isActive(item.href, item.exact) ? 'text-yt-primary' : ''}">{item.icon}</span>
          <span>{t(item.labelKey)}</span>
          {#if item.href === "/tools/ytdlp/queue" && (activeCount + pendingCount) > 0}
            <span class="absolute right-2 w-2 h-2 bg-yt-primary rounded-full ring-2 ring-yt-surface animate-pulse"></span>
          {/if}
        </a>
      {/each}
    </nav>

    <!-- GitHub Link -->
    <div class="px-3 mb-1">
      <button
        onclick={() => openUrl("https://github.com/shlifedev/Modern-YT-DLP")}
        class="flex items-center gap-3 px-3 py-2 rounded-md transition-colors text-sm font-medium text-yt-text-secondary hover:bg-yt-overlay hover:text-yt-text w-full"
      >
        <svg class="w-5 h-5 shrink-0" viewBox="0 0 24 24" fill="currentColor"><path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0024 12c0-6.63-5.37-12-12-12z"/></svg>
        <span>GitHub</span>
      </button>
    </div>

    <!-- Update Button -->
    <div class="px-3 mb-1">
      <button
        onclick={() => { showUpdateDialog = true }}
        class="flex items-center gap-3 px-3 py-2 rounded-md transition-colors text-sm font-medium text-yt-text-secondary hover:bg-yt-overlay hover:text-yt-text w-full"
      >
        <span class="material-symbols-outlined text-[20px]">system_update</span>
        <span>{t("update.checkUpdate")}</span>
        {#if updateAvailable}
          <span class="ml-auto w-2.5 h-2.5 bg-green-500 rounded-full ring-2 ring-yt-surface animate-pulse"></span>
        {/if}
      </button>
    </div>

    <!-- Sidebar Footer / Queue Summary -->
    <div class="p-3 border-t border-yt-border bg-yt-surface">
      <button 
        onclick={() => popupOpen = !popupOpen}
        class="w-full flex items-center justify-between px-3 py-2.5 rounded-lg bg-yt-highlight hover:bg-yt-overlay-strong border border-yt-border transition-all group {queueFlash ? 'animate-queue-flash' : ''}"
      >
        <div class="flex items-center gap-2.5">
          <div class="relative">
             <span class="material-symbols-outlined text-yt-text-secondary group-hover:text-yt-text text-[20px]">downloading</span>
             {#if (activeCount + pendingCount) > 0}
              <span class="absolute -top-1 -right-1 w-2.5 h-2.5 bg-yt-primary rounded-full ring-2 ring-yt-surface"></span>
             {/if}
          </div>
          <div class="text-left">
            <span class="block text-xs font-semibold text-yt-text">{t("nav.queue")}</span>
            <span class="block text-[10px] text-yt-text-secondary">
              {#if activeCount > 0}
                {t("layout.downloading", { count: activeCount })}
              {:else}
                {t("layout.idle")}
              {/if}
            </span>
          </div>
        </div>
        <span class="material-symbols-outlined text-yt-text-muted text-[16px]">expand_less</span>
      </button>
    </div>
  </aside>

  <!-- Main Content Area -->
  <main class="flex-1 flex flex-col min-w-0 bg-yt-bg relative z-0">
    <!-- Window Drag Region (Top Bar) -->
    <div data-tauri-drag-region class="h-10 shrink-0 w-full"></div>

    {#if setupCompleted === null}
      <!-- Loading settings... -->
      <div class="flex-1 flex items-center justify-center">
        <div class="flex flex-col items-center gap-3">
          <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
        </div>
      </div>
    {:else if setupCompleted === false}
      <!-- Welcome / First-run Setup -->
      <div class="flex-1 flex flex-col items-center justify-center p-8 overflow-y-auto">
        <div class="max-w-lg w-full flex flex-col items-center gap-8 animate-scale-in">
          <!-- Logo & Title -->
          <div class="flex flex-col items-center gap-4">
            <div class="w-16 h-16 rounded-2xl bg-yt-primary flex items-center justify-center text-white shadow-lg shadow-yt-primary/30">
              <span class="material-symbols-outlined text-4xl">download</span>
            </div>
            <div class="text-center space-y-2">
              <h1 class="font-display text-2xl font-bold text-yt-text">{t("welcome.title")}</h1>
              <p class="text-sm text-yt-text-secondary">{t("welcome.subtitle")}</p>
            </div>
          </div>

          <!-- Mode Selection Cards -->
          <div class="w-full space-y-3">
            <!-- App Managed -->
            <button
              onclick={() => selectedDepMode = "external"}
              class="w-full text-left p-4 rounded-xl border-2 transition-all {selectedDepMode === 'external'
                ? 'border-yt-primary bg-yt-primary/5 ring-1 ring-yt-primary/20'
                : 'border-yt-border bg-yt-surface hover:border-yt-text-secondary/30'}"
            >
              <div class="flex items-start gap-3">
                <div class="w-10 h-10 rounded-lg shrink-0 flex items-center justify-center {selectedDepMode === 'external' ? 'bg-yt-primary text-white' : 'bg-yt-highlight text-yt-text-secondary'}">
                  <span class="material-symbols-outlined text-xl">package_2</span>
                </div>
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="font-semibold text-sm text-yt-text">{t("welcome.appManaged")}</span>
                    <span class="text-[10px] font-bold uppercase px-1.5 py-0.5 rounded bg-yt-primary/15 text-yt-primary">{t("welcome.appManagedTag")}</span>
                  </div>
                  <p class="text-xs text-yt-text-secondary mt-1 leading-relaxed">{t("welcome.appManagedDesc")}</p>
                </div>
                <div class="w-5 h-5 rounded-full border-2 shrink-0 mt-0.5 flex items-center justify-center {selectedDepMode === 'external' ? 'border-yt-primary' : 'border-yt-border'}">
                  {#if selectedDepMode === "external"}
                    <div class="w-2.5 h-2.5 rounded-full bg-yt-primary"></div>
                  {/if}
                </div>
              </div>
            </button>

            <!-- System PATH -->
            <button
              onclick={() => selectedDepMode = "system"}
              class="w-full text-left p-4 rounded-xl border-2 transition-all {selectedDepMode === 'system'
                ? 'border-yt-primary bg-yt-primary/5 ring-1 ring-yt-primary/20'
                : 'border-yt-border bg-yt-surface hover:border-yt-text-secondary/30'}"
            >
              <div class="flex items-start gap-3">
                <div class="w-10 h-10 rounded-lg shrink-0 flex items-center justify-center {selectedDepMode === 'system' ? 'bg-yt-primary text-white' : 'bg-yt-highlight text-yt-text-secondary'}">
                  <span class="material-symbols-outlined text-xl">terminal</span>
                </div>
                <div class="flex-1 min-w-0">
                  <span class="font-semibold text-sm text-yt-text">{t("welcome.systemPath")}</span>
                  <p class="text-xs text-yt-text-secondary mt-1 leading-relaxed">{t("welcome.systemPathDesc")}</p>
                </div>
                <div class="w-5 h-5 rounded-full border-2 shrink-0 mt-0.5 flex items-center justify-center {selectedDepMode === 'system' ? 'border-yt-primary' : 'border-yt-border'}">
                  {#if selectedDepMode === "system"}
                    <div class="w-2.5 h-2.5 rounded-full bg-yt-primary"></div>
                  {/if}
                </div>
              </div>
            </button>
          </div>

          <!-- Start Button -->
          <button
            onclick={handleWelcomeComplete}
            disabled={!selectedDepMode}
            class="w-full py-3 rounded-xl bg-yt-primary hover:bg-yt-primary-hover text-white text-sm font-semibold transition-all shadow-sm disabled:opacity-40 disabled:cursor-not-allowed flex items-center justify-center gap-2"
          >
            <span class="material-symbols-outlined text-[18px]">arrow_forward</span>
            {t("welcome.start")}
          </button>
        </div>
      </div>
    {:else if checking}
      <div class="flex-1 flex items-center justify-center">
        <div class="flex flex-col items-center gap-3">
          <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
          <p class="text-sm text-yt-text-secondary">{t("layout.checkingDeps")}</p>
        </div>
      </div>
    {:else if !depsInstalled}
      <div class="flex-1 flex flex-col items-center justify-center p-8 overflow-y-auto">
        <div class="max-w-xl w-full flex flex-col items-center gap-6">
           <div class="w-16 h-16 rounded-2xl bg-yt-surface border border-yt-border flex items-center justify-center shadow-sm">
            <span class="material-symbols-outlined text-yt-primary text-4xl">download</span>
          </div>

          <div class="text-center space-y-2">
            <h2 class="font-display text-xl font-semibold text-yt-text">
              {currentDepMode === "system" ? t("layout.systemPathMissing") : t("layout.setupRequired")}
            </h2>
            <p class="text-yt-text-secondary text-sm leading-relaxed">
              {currentDepMode === "system" ? t("layout.systemPathMissingDesc") : t("layout.setupDesc")}
            </p>
          </div>

          <!-- Dependencies Cards -->
          <div class="grid grid-cols-3 gap-3 w-full">
            {#each [
              { name: "yt-dlp", installed: fullDepStatus?.ytdlp?.installed ?? ytdlpInstalled, version: fullDepStatus?.ytdlp?.version ?? ytdlpVersion, source: fullDepStatus?.ytdlp?.source },
              { name: "ffmpeg", installed: fullDepStatus?.ffmpeg?.installed ?? ffmpegInstalled, version: fullDepStatus?.ffmpeg?.version ?? ffmpegVersion, source: fullDepStatus?.ffmpeg?.source },
              { name: "deno", installed: fullDepStatus?.deno?.installed ?? false, version: fullDepStatus?.deno?.version, source: fullDepStatus?.deno?.source },
            ] as dep}
              <div class="bg-yt-surface border border-yt-border rounded-lg p-3 flex flex-col gap-2">
                <div class="flex items-center gap-2">
                  <span class="material-symbols-outlined text-[18px] {dep.installed ? 'text-yt-success' : 'text-yt-error'}">
                    {dep.installed ? "check_circle" : "cancel"}
                  </span>
                  <p class="text-xs font-semibold text-yt-text">{dep.name}</p>
                </div>
                <p class="text-[10px] truncate opacity-70">
                  {#if dep.installed}
                    {dep.version || t("layout.installed")}
                  {:else}
                    {t("layout.missing")}
                  {/if}
                </p>
                {#if installing && installProgress[dep.name]}
                  <div class="mt-1">
                    <div class="h-1 bg-yt-border rounded-full overflow-hidden">
                      <div
                        class="h-full bg-yt-primary transition-all duration-300 rounded-full"
                        style="width: {installProgress[dep.name].percent}%"
                      ></div>
                    </div>
                    <p class="text-[9px] text-yt-text-muted mt-1">
                      {installProgress[dep.name].stage === "Downloading" ? t("layout.depDownloading") : ""}
                      {installProgress[dep.name].stage === "Extracting" ? t("layout.extracting") : ""}
                      {installProgress[dep.name].stage === "Verifying" ? t("layout.verifying") : ""}
                      {installProgress[dep.name].stage === "Completing" ? t("layout.installSuccess") : ""}
                      {installProgress[dep.name].stage === "Failed" ? t("layout.installFailed") : ""}
                      {installProgress[dep.name].percent > 0 ? ` ${installProgress[dep.name].percent.toFixed(0)}%` : ""}
                    </p>
                  </div>
                {/if}
              </div>
            {/each}
          </div>

          {#if installError}
            <div class="w-full bg-red-500/10 border border-red-500/30 rounded-lg p-3">
              <p class="text-xs text-red-400 font-mono whitespace-pre-wrap">{installError}</p>
            </div>
          {/if}

          <div class="w-full space-y-3">
            {#if currentDepMode === "system"}
              <!-- System PATH mode: show manual install commands expanded by default -->
              <div class="space-y-3">
                <div>
                  <div class="flex items-center justify-between mb-2">
                    <span class="text-xs font-medium text-yt-text">{t("layout.recommendedCommand")}</span>
                    <span class="text-[10px] text-yt-text-muted bg-yt-surface border border-yt-border px-1.5 py-0.5 rounded uppercase">{currentPlatform}</span>
                  </div>
                  <div class="relative group">
                    <code class="block w-full bg-yt-surface border border-yt-border rounded-lg p-3 text-xs font-mono text-yt-text select-all">
                      {platformCommands.recommended}
                    </code>
                    <button
                      onclick={() => copyCommand(platformCommands.recommended)}
                      class="absolute right-2 top-2 p-1 rounded hover:bg-yt-highlight text-yt-text-secondary transition-colors"
                    >
                      <span class="material-symbols-outlined text-[16px]">{copiedCmd === platformCommands.recommended ? 'check' : 'content_copy'}</span>
                    </button>
                  </div>
                </div>
              </div>

              <!-- Recheck Button -->
              <button
                class="w-full py-2.5 rounded-lg bg-yt-primary hover:bg-yt-primary-hover text-white text-sm font-medium transition-colors shadow-sm flex items-center justify-center gap-2"
                onclick={() => checkDeps(true)}
              >
                <span class="material-symbols-outlined text-[18px]">refresh</span>
                {t("layout.recheck")}
              </button>

              <!-- Divider -->
              <div class="flex items-center gap-3 py-1">
                <div class="flex-1 h-px bg-yt-border"></div>
                <span class="text-[10px] text-yt-text-muted uppercase tracking-wider">{t("layout.altMethod")}</span>
                <div class="flex-1 h-px bg-yt-border"></div>
              </div>

              <!-- Switch to App Managed -->
              <div class="bg-yt-surface border border-yt-border rounded-lg p-3">
                <p class="text-xs text-yt-text-secondary mb-2">{t("layout.switchToAppManagedDesc")}</p>
                <button
                  class="w-full py-2 rounded-lg bg-yt-highlight hover:bg-yt-overlay-strong border border-yt-border text-yt-text text-sm font-medium transition-colors flex items-center justify-center gap-2"
                  onclick={handleSwitchToAppManaged}
                  disabled={installing}
                >
                  {#if installing}
                    <span class="material-symbols-outlined text-[18px] animate-spin">progress_activity</span>
                    {t("layout.installing")}
                  {:else}
                    <span class="material-symbols-outlined text-[18px]">package_2</span>
                    {t("layout.switchToAppManaged")}
                  {/if}
                </button>
              </div>
            {:else}
              <!-- App Managed mode: Auto Install Button -->
              <button
                class="w-full py-2.5 rounded-lg bg-yt-primary hover:bg-yt-primary-hover text-white text-sm font-medium transition-colors shadow-sm disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                onclick={handleAutoInstall}
                disabled={installing}
              >
                {#if installing}
                  <span class="material-symbols-outlined text-[18px] animate-spin">progress_activity</span>
                  {t("layout.installing")}
                {:else}
                  <span class="material-symbols-outlined text-[18px]">download</span>
                  {t("layout.autoInstall")}
                {/if}
              </button>

              <!-- Recheck Button -->
              <button
                class="w-full py-2 rounded-lg bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text text-sm font-medium transition-colors"
                onclick={() => checkDeps(true)}
                disabled={installing}
              >
                {t("layout.recheck")}
              </button>

              <!-- Manual Install Toggle -->
              <button
                class="w-full text-xs text-yt-text-secondary hover:text-yt-text transition-colors flex items-center justify-center gap-1"
                onclick={() => showManualInstall = !showManualInstall}
              >
                <span class="material-symbols-outlined text-[14px]">{showManualInstall ? "expand_less" : "expand_more"}</span>
                {t("layout.manualInstall")}
              </button>

              {#if showManualInstall}
                <div class="space-y-3 animate-scale-in">
                  <div>
                    <div class="flex items-center justify-between mb-2">
                      <span class="text-xs font-medium text-yt-text">{t("layout.recommendedCommand")}</span>
                      <span class="text-[10px] text-yt-text-muted bg-yt-surface border border-yt-border px-1.5 py-0.5 rounded uppercase">{currentPlatform}</span>
                    </div>
                    <div class="relative group">
                      <code class="block w-full bg-yt-surface border border-yt-border rounded-lg p-3 text-xs font-mono text-yt-text select-all">
                        {platformCommands.recommended}
                      </code>
                      <button
                        onclick={() => copyCommand(platformCommands.recommended)}
                        class="absolute right-2 top-2 p-1 rounded hover:bg-yt-highlight text-yt-text-secondary transition-colors"
                      >
                        <span class="material-symbols-outlined text-[16px]">{copiedCmd === platformCommands.recommended ? 'check' : 'content_copy'}</span>
                      </button>
                    </div>
                  </div>
                </div>
              {/if}
            {/if}
          </div>
        </div>
      </div>
    {:else}
      <div class="flex-1 overflow-hidden relative">
        {@render children()}
      </div>
    {/if}
  </main>

  <!-- Download Popup (Left aligned to sidebar bottom now, or centered? Let's keep it fixed absolute relative to screen or button) -->
  {#if popupOpen}
    <!-- Backdrop -->
    <button
      class="fixed inset-0 bg-black/20 z-40"
      onclick={() => popupOpen = false}
      aria-label="Close popup"
    ></button>

    <!-- Floating Popup (Anchored near bottom left sidebar) -->
    <div class="fixed bottom-16 left-4 w-80 max-h-[60vh] bg-yt-surface rounded-xl shadow-2xl ring-1 ring-black/5 z-50 flex flex-col animate-popup-in">
       <div class="p-3 border-b border-yt-border flex items-center justify-between bg-yt-surface rounded-t-xl">
        <h3 class="font-semibold text-xs text-yt-text px-1">{t("layout.recentActivity")}</h3>
        <div class="flex items-center gap-1">
          {#if (activeCount + pendingCount) > 0}
            <button
              onclick={handleCancelAll}
              class="text-yt-error hover:bg-yt-error/10 text-[10px] font-medium px-2 py-1 rounded transition-colors"
            >
              {t("layout.stopAll")}
            </button>
          {/if}
        </div>
      </div>

      <div class="flex-1 overflow-y-auto hide-scrollbar p-2 space-y-2">
        {#if activeDownloads.length === 0 && recentCompleted.length === 0}
           <div class="py-8 text-center">
             <p class="text-xs text-yt-text-muted">{t("layout.noActiveDownloads")}</p>
           </div>
        {/if}

        {#each activeDownloads as item (item.id)}
          <div class="bg-yt-bg rounded border border-yt-border p-2.5 relative overflow-hidden group">
            <p class="text-xs font-medium text-yt-text truncate relative z-10">{item.title}</p>
            <div class="flex items-center justify-between mt-1.5 relative z-10">
              <span class="text-[10px] text-yt-text-secondary font-mono">{(item.progress || 0).toFixed(0)}%</span>
              <span class="text-[10px] text-yt-text-muted">{item.speed || ""}</span>
            </div>
            <!-- Progress Bar Background -->
            <div class="absolute bottom-0 left-0 h-0.5 bg-yt-primary/20 w-full">
              <div class="h-full bg-yt-primary transition-all duration-300" style="width: {item.progress || 0}%"></div>
            </div>
          </div>
        {/each}

        {#if recentCompleted.length > 0}
          <div class="pt-2">
            <div class="text-[10px] font-semibold text-yt-text-muted uppercase tracking-wider mb-2 px-1">{t("layout.recentlyCompleted")}</div>
            <div class="space-y-1">
              {#each recentCompleted as item}
                <div class="flex items-center gap-2 px-2 py-1.5 rounded hover:bg-yt-highlight transition-colors">
                  <span class="material-symbols-outlined text-yt-success text-[14px]">check_circle</span>
                  <span class="text-xs text-yt-text-secondary truncate flex-1">{item.title}</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>
      
       <div class="p-2 border-t border-yt-border">
        <a href="/tools/ytdlp/queue" class="flex items-center justify-center gap-1 w-full py-1.5 text-xs font-medium text-yt-text-secondary hover:text-yt-text hover:bg-yt-highlight rounded transition-colors" onclick={() => popupOpen = false}>
          {t("layout.viewFullHistory")}
        </a>
      </div>
    </div>
  {/if}

  <!-- Toast Notification -->
  <!-- Toast Notification -->
  {#if toastVisible}
    <div class="fixed top-6 left-1/2 -translate-x-1/2 z-[200] animate-toast-in">
      <div class="flex items-center gap-4 bg-yt-surface border border-yt-border border-l-4 {toastType === 'error' ? 'border-l-yt-error' : 'border-l-yt-success'} text-yt-text px-6 py-4 rounded-lg shadow-2xl">
        <span class="material-symbols-outlined text-[24px] {toastType === 'error' ? 'text-yt-error' : 'text-yt-success'}">{toastIcon}</span>
        <span class="text-base font-medium">{toastMessage}</span>
      </div>
    </div>
  {/if}

  <!-- Update Dialog -->
  {#if showUpdateDialog}
    <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/20 backdrop-blur-sm">
      <div class="bg-yt-surface border border-yt-border rounded-xl shadow-2xl p-6 max-w-md w-full mx-4 animate-scale-in">
        <div class="flex items-center gap-3 mb-4">
          <div class="w-10 h-10 rounded-lg bg-yt-primary/10 flex items-center justify-center">
            <span class="material-symbols-outlined text-yt-primary text-2xl">system_update</span>
          </div>
          <div>
            <h3 class="font-display font-semibold text-lg text-yt-text">
              {updateAvailable ? t("update.available") : t("update.checkUpdate")}
            </h3>
            {#if updateAvailable && updateInfo}
              <p class="text-xs text-yt-text-secondary">{t("update.version", { version: updateInfo.version })}</p>
            {/if}
          </div>
        </div>

        {#if updateAvailable && updateInfo}
          {#if updateInfo.body}
            <div class="mb-4">
              <p class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider mb-2">{t("update.releaseNotes")}</p>
              <div class="bg-yt-bg border border-yt-border rounded-lg p-3 max-h-40 overflow-y-auto">
                <p class="text-xs text-yt-text-secondary whitespace-pre-wrap">{updateInfo.body}</p>
              </div>
            </div>
          {/if}

          {#if updateDownloading}
            <div class="mb-4">
              <div class="flex items-center justify-between mb-2">
                <p class="text-xs text-yt-text-secondary">
                  {updateReady ? t("update.readyToInstall") : t("update.downloading")}
                </p>
                <p class="text-xs font-mono text-yt-text-secondary">{updateProgress}%</p>
              </div>
              <div class="h-2 bg-yt-border rounded-full overflow-hidden">
                <div
                  class="h-full bg-yt-primary transition-all duration-300 rounded-full"
                  style="width: {updateProgress}%"
                ></div>
              </div>
            </div>
          {/if}

          <div class="flex gap-3">
            {#if !updateDownloading}
              <button
                onclick={() => { showUpdateDialog = false }}
                class="flex-1 px-4 py-2 rounded-lg bg-yt-highlight hover:bg-yt-border text-yt-text text-sm font-medium transition-colors"
              >
                {t("update.later")}
              </button>
              <button
                onclick={handleUpdate}
                class="flex-1 px-4 py-2 rounded-lg bg-yt-primary hover:bg-yt-primary-hover text-white text-sm font-medium transition-colors flex items-center justify-center gap-2"
              >
                <span class="material-symbols-outlined text-[18px]">download</span>
                {t("update.install")}
              </button>
            {:else}
              <button
                disabled
                class="flex-1 px-4 py-2 rounded-lg bg-yt-highlight text-yt-text-secondary text-sm font-medium cursor-not-allowed flex items-center justify-center gap-2"
              >
                <span class="material-symbols-outlined text-[18px] animate-spin">progress_activity</span>
                {t("update.downloading")}
              </button>
            {/if}
          </div>
        {:else}
          <p class="text-sm text-yt-text-secondary mb-4">{t("update.upToDate")}</p>
          <button
            onclick={() => { showUpdateDialog = false }}
            class="w-full px-4 py-2 rounded-lg bg-yt-highlight hover:bg-yt-border text-yt-text text-sm font-medium transition-colors"
          >
            {t("update.later")}
          </button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Close Dialog -->
  {#if showCloseDialog}
    <div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/20 backdrop-blur-sm">
      <div class="bg-yt-surface border border-yt-border rounded-xl shadow-2xl p-6 max-w-sm w-full mx-4 animate-scale-in">
        <h3 class="font-display font-semibold text-lg text-yt-text mb-2">{t("tray.dialogTitle")}</h3>
        <p class="text-sm text-yt-text-secondary mb-6">{t("tray.dialogMessage")}</p>

        <label class="flex items-center gap-2 mb-6 cursor-pointer select-none">
          <input type="checkbox" bind:checked={rememberChoice} class="rounded border-yt-border bg-yt-bg text-yt-primary focus:ring-yt-primary" />
          <span class="text-sm text-yt-text">{t("tray.rememberChoice")}</span>
        </label>

        <div class="flex gap-3">
          <button
            onclick={() => handleCloseChoice(false)}
            class="flex-1 px-4 py-2 rounded-lg bg-yt-highlight hover:bg-yt-border text-yt-text text-sm font-medium transition-colors"
          >
            {t("tray.quit")}
          </button>
          <button
            onclick={() => handleCloseChoice(true)}
            class="flex-1 px-4 py-2 rounded-lg bg-yt-primary hover:bg-yt-primary-hover text-white text-sm font-medium transition-colors"
          >
             {t("tray.minimize")}
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Debug Overlay (F10) -->
  {#if showDebug}
    <div class="fixed inset-0 z-[101] flex items-center justify-center p-8">
      <!-- Backdrop -->
      <button 
        class="absolute inset-0 bg-black/50 backdrop-blur-sm border-none cursor-default w-full h-full"
        onclick={() => showDebug = false} 
        onkeydown={(e) => e.key === 'Escape' && (showDebug = false)}
        aria-label="Close Debug Logs"
      ></button>

      <!-- Modal -->
      <div 
        class="relative bg-yt-surface border border-yt-border rounded-xl shadow-2xl w-full max-w-3xl max-h-full flex flex-col overflow-hidden text-left" 
        role="dialog" 
        aria-modal="true" 
        aria-label="Debug Logs"
      >
         <div class="px-4 py-3 border-b border-yt-border flex items-center justify-between bg-yt-surface">
            <h3 class="font-mono text-sm font-bold text-yt-text">Debug Logs</h3>
             <button onclick={copyLogs} class="text-xs font-medium text-yt-primary hover:underline">
               {logsCopied ? "Copied!" : "Copy to Clipboard"}
             </button>
         </div>
         <div class="flex-1 overflow-auto bg-yt-bg p-4 font-mono text-xs text-yt-text-secondary">
            {#if recentLogs}
              <pre class="whitespace-pre-wrap">{recentLogs}</pre>
            {:else}
              <div class="text-center py-10 opacity-50">No logs available</div>
            {/if}
             {#if ytdlpDebug}
              <div class="mt-4 pt-4 border-t border-yt-border border-dashed">
                 <div class="font-bold mb-2">Environment Check:</div>
                 <pre class="whitespace-pre-wrap">{ytdlpDebug}</pre>
              </div>
            {/if}
         </div>
      </div>
    </div>
  {/if}

  <!-- F9 Debug Command Menu -->
  {#if showDebugCmd}
    <div class="fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-sm" role="dialog" aria-modal="true" aria-label="Debug Commands">
      <div class="w-[480px] max-h-[80vh] bg-yt-surface rounded-xl shadow-2xl border border-yt-border flex flex-col animate-scale-in">
        <div class="px-5 py-4 border-b border-yt-border flex items-center justify-between">
          <div class="flex items-center gap-2">
            <span class="material-symbols-outlined text-xl text-yt-primary">terminal</span>
            <h3 class="font-mono text-sm font-bold text-yt-text">Debug Commands</h3>
            <span class="text-[10px] font-mono bg-yt-highlight text-yt-text-secondary px-1.5 py-0.5 rounded">F9</span>
          </div>
          <button onclick={() => showDebugCmd = false} class="text-yt-text-secondary hover:text-yt-text transition-colors">
            <span class="material-symbols-outlined text-xl">close</span>
          </button>
        </div>

        <div class="flex-1 overflow-auto p-5 space-y-4">
          <!-- Dependency Status -->
          <div class="space-y-1">
            <div class="flex items-center justify-between">
              <h4 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider">App-Managed Dependencies</h4>
              <button
                onclick={debugRefreshStatus}
                disabled={debugDepLoading}
                class="text-xs text-yt-primary hover:underline disabled:opacity-50 flex items-center gap-1"
              >
                <span class="material-symbols-outlined text-sm {debugDepLoading ? 'animate-spin' : ''}">refresh</span>
                Refresh
              </button>
            </div>
          </div>

          {#if debugDepStatus}
            {#each [
              { key: "yt-dlp", info: debugDepStatus.ytdlp },
              { key: "ffmpeg", info: debugDepStatus.ffmpeg },
              { key: "deno", info: debugDepStatus.deno },
            ] as dep}
              <div class="bg-yt-bg rounded-lg border border-yt-border p-3">
                <div class="flex items-center justify-between mb-2">
                  <div class="flex items-center gap-2">
                    <span class="material-symbols-outlined text-base {dep.info.installed ? 'text-green-500' : 'text-red-400'}">
                      {dep.info.installed ? "check_circle" : "cancel"}
                    </span>
                    <span class="font-mono text-sm font-semibold text-yt-text">{dep.key}</span>
                    {#if dep.info.version}
                      <span class="text-[10px] font-mono text-yt-text-secondary bg-yt-highlight px-1.5 py-0.5 rounded">{dep.info.version}</span>
                    {/if}
                  </div>
                  <span class="text-[10px] font-mono px-1.5 py-0.5 rounded {dep.info.source === 'AppManaged' ? 'bg-blue-500/20 text-blue-400' : dep.info.source === 'SystemPath' ? 'bg-green-500/20 text-green-400' : 'bg-red-500/20 text-red-400'}">
                    {dep.info.source}
                  </span>
                </div>
                {#if dep.info.path}
                  <div class="text-[10px] font-mono text-yt-text-secondary truncate mb-2" title={dep.info.path}>{dep.info.path}</div>
                {/if}
                <div class="flex items-center gap-2">
                  <button
                    onclick={() => debugDeleteDep(dep.key)}
                    disabled={debugCmdResults[dep.key]?.status === "loading" || dep.info.source !== "AppManaged"}
                    class="text-xs px-3 py-1.5 rounded-md font-medium transition-colors
                      {dep.info.source === 'AppManaged'
                        ? 'bg-red-500/20 text-red-400 hover:bg-red-500/30 disabled:opacity-50'
                        : 'bg-yt-highlight text-yt-text-secondary cursor-not-allowed opacity-40'}"
                  >
                    {#if debugCmdResults[dep.key]?.status === "loading"}
                      <span class="flex items-center gap-1">
                        <span class="material-symbols-outlined text-sm animate-spin">progress_activity</span>
                        Deleting...
                      </span>
                    {:else}
                      Delete Binary
                    {/if}
                  </button>
                  {#if debugCmdResults[dep.key]?.status === "success"}
                    <span class="text-[11px] text-green-400 flex items-center gap-1">
                      <span class="material-symbols-outlined text-sm">check</span>
                      {debugCmdResults[dep.key].message}
                    </span>
                  {:else if debugCmdResults[dep.key]?.status === "error"}
                    <span class="text-[11px] text-red-400 flex items-center gap-1">
                      <span class="material-symbols-outlined text-sm">error</span>
                      {debugCmdResults[dep.key].message}
                    </span>
                  {/if}
                </div>
              </div>
            {/each}
          {:else}
            <div class="text-center py-6 text-yt-text-secondary text-sm">
              {#if debugDepLoading}
                <span class="material-symbols-outlined text-2xl animate-spin mb-2">progress_activity</span>
                <div>Loading status...</div>
              {:else}
                <span class="material-symbols-outlined text-2xl mb-2 opacity-50">info</span>
                <div>Click "Refresh" to load dependency status</div>
              {/if}
            </div>
          {/if}
        </div>

        <!-- Factory Reset Section -->
        <div class="px-5 pb-4">
          <div class="border-t border-yt-border pt-4 mt-0">
            <div class="flex items-center gap-2 mb-2">
              <span class="material-symbols-outlined text-red-400 text-base">warning</span>
              <h4 class="text-xs font-semibold text-red-400 uppercase tracking-wider">{t("debug.resetAll")}</h4>
            </div>
            <p class="text-[11px] text-yt-text-secondary mb-3">{t("debug.resetAllDesc")}</p>
            {#if !resetConfirming}
              <button
                onclick={() => resetConfirming = true}
                class="w-full py-2 rounded-lg bg-red-500/15 hover:bg-red-500/25 text-red-400 text-xs font-semibold transition-colors border border-red-500/20"
              >
                {t("debug.resetAll")}
              </button>
            {:else if resetting}
              <button disabled class="w-full py-2 rounded-lg bg-red-500/15 text-red-400 text-xs font-semibold opacity-60 cursor-not-allowed flex items-center justify-center gap-2">
                <span class="material-symbols-outlined text-sm animate-spin">progress_activity</span>
                {t("debug.resetting")}
              </button>
            {:else}
              <div class="space-y-2">
                <p class="text-[11px] text-red-400 font-medium">{t("debug.resetConfirm")}</p>
                <div class="flex gap-2">
                  <button
                    onclick={() => resetConfirming = false}
                    class="flex-1 py-2 rounded-lg bg-yt-highlight hover:bg-yt-border text-yt-text text-xs font-medium transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onclick={handleFactoryReset}
                    class="flex-1 py-2 rounded-lg bg-red-600 hover:bg-red-700 text-white text-xs font-semibold transition-colors"
                  >
                    {t("debug.resetAll")}
                  </button>
                </div>
              </div>
            {/if}
          </div>
        </div>

        <div class="px-5 py-3 border-t border-yt-border bg-yt-bg/50">
          <p class="text-[10px] text-yt-text-secondary font-mono">
            Deleting app-managed binaries will require re-installation on next app launch.
          </p>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  @keyframes popup-in {
    from { opacity: 0; transform: translateY(8px); }
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
    animation: toast-in 0.2s ease-out cubic-bezier(0.16, 1, 0.3, 1);
  }
  
  @keyframes scale-in {
    from { opacity: 0; transform: scale(0.95); }
    to { opacity: 1; transform: scale(1); }
  }
  .animate-scale-in {
    animation: scale-in 0.15s ease-out;
  }

  @keyframes queue-flash {
    0% { transform: scale(1); background-color: var(--color-yt-highlight); }
    50% { transform: scale(1.05); background-color: var(--color-yt-primary); color: white; }
    100% { transform: scale(1); background-color: var(--color-yt-highlight); }
  }
  .animate-queue-flash {
    animation: queue-flash 0.4s ease-out;
  }
</style>
