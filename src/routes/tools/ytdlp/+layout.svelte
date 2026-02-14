<script lang="ts">
  import { commands } from "$lib/bindings"
  import { Channel } from "@tauri-apps/api/core"
  import { listen } from "@tauri-apps/api/event"
  import { page } from "$app/stores"
  import { onMount, onDestroy } from "svelte"

  let { children } = $props()

  let checking = $state(true)
  let depsInstalled = $state(false)
  let installing = $state(false)
  let installMessage = $state("")
  let installError = $state("")

  // Popup state
  let popupOpen = $state(false)
  let activeDownloads = $state<any[]>([])
  let recentCompleted = $state<any[]>([])
  let activeCount = $derived(activeDownloads.filter(d => d.status === "downloading").length)
  let pendingCount = $derived(activeDownloads.filter(d => d.status === "pending").length)

  const navItems = [
    { href: "/tools/ytdlp/history", label: "Library", icon: "library_books" },
    { href: "/tools/ytdlp/settings", label: "Settings", icon: "settings" },
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

  onMount(async () => {
    await checkDeps()

    try {
      const unlistenFn = await listen("download-event", () => {
        loadActiveDownloads()
      })
      unlisten = unlistenFn
    } catch (e) { console.error("Failed to listen for download events:", e) }

    loadActiveDownloads()
  })

  onDestroy(() => {
    stopPopupRefresh()
    if (unlisten) unlisten()
  })

  async function checkDeps() {
    checking = true
    try {
      const result = await commands.checkDependencies()
      if (result.status === "ok") {
        depsInstalled = result.data.ytdlpInstalled
      }
    } catch (e) {
      console.error(e)
    } finally {
      checking = false
    }
  }

  async function handleInstall() {
    installing = true
    installError = ""
    installMessage = "yt-dlp 다운로드 중..."

    try {
      const channel = new Channel()
      channel.onmessage = (event: any) => {
        if (event.event === "progress") {
          installMessage = event.data.message
        } else if (event.event === "completed") {
          installMessage = event.data.message
        } else if (event.event === "error") {
          installError = event.data.message
        }
      }

      const result = await commands.installDependencies(channel)
      if (result.status === "error") {
        installError = JSON.stringify(result.error)
      } else {
        await checkDeps()
      }
    } catch (e: any) {
      installError = e.message || String(e)
    } finally {
      installing = false
    }
  }
</script>

<div class="flex flex-col h-screen overflow-hidden bg-yt-bg">
  <!-- Top Header Bar -->
  <header class="h-14 bg-yt-surface border-b border-gray-200 flex items-center justify-between px-5 shrink-0 z-30">
    <!-- Left: Logo -->
    <a href="/tools/ytdlp" class="flex items-center gap-3 hover:opacity-80 transition-opacity">
      <div class="w-8 h-8 rounded-lg bg-yt-primary flex items-center justify-center text-white shrink-0">
        <span class="material-symbols-outlined text-[20px]">download</span>
      </div>
      <h1 class="font-display font-bold text-base text-gray-900">YT-DLP GUI</h1>
    </a>

    <!-- Right: Actions -->
    <div class="flex items-center gap-1">
      <!-- Queue Popup Toggle -->
      <div class="relative">
        <button
          onclick={() => popupOpen = !popupOpen}
          class="flex items-center gap-2 px-3 py-2 rounded-lg text-gray-500 hover:bg-gray-100 hover:text-gray-900 transition-colors relative"
          title="Active Downloads"
        >
          <span class="material-symbols-outlined text-[20px]">downloading</span>
          <span class="text-sm hidden sm:inline">Queue</span>
          {#if (activeCount + pendingCount) > 0}
            <span class="absolute top-1 right-1 w-5 h-5 bg-yt-primary text-white text-[10px] font-bold rounded-full flex items-center justify-center">
              {activeCount + pendingCount}
            </span>
          {/if}
        </button>
      </div>

      <div class="h-6 w-px bg-gray-200 mx-1"></div>

      {#each navItems as item}
        <a
          href={item.href}
          class="flex items-center gap-2 px-3 py-2 rounded-lg transition-colors text-sm
            {isActive(item.href) ? 'bg-yt-primary/10 text-yt-primary font-medium' : 'text-gray-500 hover:bg-gray-100 hover:text-gray-900'}"
        >
          <span class="material-symbols-outlined text-[20px]">{item.icon}</span>
          <span class="hidden sm:inline">{item.label}</span>
        </a>
      {/each}
    </div>
  </header>

  <!-- Main Content -->
  <main class="flex-1 flex overflow-hidden relative">
    <div class="absolute top-0 left-0 w-full h-[500px] bg-gradient-to-b from-yt-primary/5 to-transparent pointer-events-none z-0"></div>

    {#if checking}
      <div class="flex-1 flex items-center justify-center z-10">
        <div class="flex flex-col items-center gap-3">
          <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
          <p class="text-sm text-gray-400">의존성 확인 중...</p>
        </div>
      </div>
    {:else if !depsInstalled}
      <div class="flex-1 flex flex-col items-center justify-center z-10 gap-6">
        <div class="w-16 h-16 rounded-xl bg-yt-primary/20 flex items-center justify-center">
          <span class="material-symbols-outlined text-yt-primary text-4xl">download</span>
        </div>
        <h2 class="font-display text-2xl font-bold text-gray-900">yt-dlp 설정 필요</h2>
        <p class="text-gray-400">YouTube 다운로드를 위해 yt-dlp를 설치해야 합니다.</p>

        {#if installError}
          <div class="bg-red-500/10 border border-red-500/20 rounded-xl px-6 py-3 text-red-600 text-sm max-w-md">
            {installError}
          </div>
        {/if}

        {#if installing}
          <div class="flex flex-col items-center gap-3">
            <span class="material-symbols-outlined text-yt-primary text-3xl animate-spin">progress_activity</span>
            <p class="text-sm text-gray-400">{installMessage}</p>
          </div>
        {:else}
          <button
            class="px-8 py-3 rounded-xl bg-yt-primary hover:bg-blue-600 text-white font-bold transition-all shadow-lg shadow-yt-primary/20"
            onclick={handleInstall}
          >
            yt-dlp 설치하기
          </button>
        {/if}
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
      class="fixed inset-0 bg-black/20 z-40"
      onclick={() => popupOpen = false}
      aria-label="Close popup"
    ></button>

    <!-- Floating Popup -->
    <div class="fixed top-14 right-4 w-96 max-h-[70vh] bg-yt-surface rounded-xl shadow-2xl z-50 flex flex-col border border-gray-200 animate-popup-in">
      <!-- Header -->
      <div class="px-4 py-3 border-b border-gray-200 flex items-center justify-between shrink-0">
        <h3 class="font-display font-semibold text-sm text-gray-900">Queue</h3>
        <div class="flex items-center gap-1">
          {#if (activeCount + pendingCount) > 0}
            <button
              onclick={handleCancelAll}
              class="text-amber-600 hover:bg-amber-500/10 text-xs font-medium px-2 py-1 rounded-lg transition-colors"
            >
              Cancel All
            </button>
          {/if}
          <button onclick={() => popupOpen = false} class="text-gray-400 hover:text-gray-600 transition-colors p-1 rounded-lg hover:bg-gray-100">
            <span class="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>
      </div>

      <!-- Active Downloads -->
      <div class="flex-1 overflow-y-auto hide-scrollbar">
        {#if activeDownloads.length === 0}
          <div class="flex flex-col items-center justify-center py-12 text-center px-4">
            <span class="material-symbols-outlined text-gray-300 text-4xl">cloud_done</span>
            <p class="text-gray-400 text-sm mt-2">활성 다운로드가 없습니다</p>
          </div>
        {:else}
          <div class="p-3 space-y-2">
            {#each activeDownloads as item}
              <div class="bg-yt-highlight rounded-lg p-3 border border-gray-200 {item.status === 'downloading' ? '!border-yt-primary/30' : ''}">
                <p class="text-sm text-gray-900 truncate font-medium">{item.title}</p>
                <div class="flex items-center justify-between mt-1.5">
                  {#if item.status === "downloading"}
                    <span class="text-xs text-yt-primary font-mono">{(item.progress || 0).toFixed(0)}%</span>
                    <span class="text-xs text-gray-400">{item.speed || "..."}</span>
                  {:else}
                    <span class="inline-flex items-center gap-1 text-xs text-amber-600 bg-amber-500/10 px-2 py-0.5 rounded-full">
                      <span class="material-symbols-outlined text-[14px]">schedule</span>
                      Queued
                    </span>
                  {/if}
                </div>
                {#if item.status === "downloading"}
                  <div class="w-full bg-gray-200 rounded-full h-1 mt-2">
                    <div class="bg-yt-primary h-1 rounded-full transition-all" style="width: {item.progress || 0}%"></div>
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        <!-- Recent Completed -->
        {#if recentCompleted.length > 0}
          <div class="border-t border-gray-200 mt-2">
            <div class="px-4 py-2">
              <h4 class="text-xs text-gray-400 font-medium uppercase tracking-wider">Recent</h4>
            </div>
            <div class="px-3 pb-3 space-y-1">
              {#each recentCompleted as item}
                <div class="flex items-center gap-2 px-2 py-1.5 rounded-lg">
                  <span class="material-symbols-outlined text-green-600 text-[16px]">check_circle</span>
                  <span class="text-sm text-gray-600 truncate">{item.title}</span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <!-- Footer: View All -->
      <div class="border-t border-gray-200 px-4 py-2.5 shrink-0">
        <a href="/tools/ytdlp/queue" class="flex items-center justify-center gap-1.5 text-sm text-yt-primary hover:text-blue-700 font-medium transition-colors" onclick={() => popupOpen = false}>
          <span>View All</span>
          <span class="material-symbols-outlined text-[16px]">arrow_forward</span>
        </a>
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
</style>
