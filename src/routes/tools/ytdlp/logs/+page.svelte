<script lang="ts">
  import { commands } from "$lib/bindings"
  import type { LogEntry, LogStats } from "$lib/bindings"
  import { onMount, onDestroy } from "svelte"
  import { listen } from "@tauri-apps/api/event"
  import { t } from "$lib/i18n/index.svelte"

  let logs = $state<LogEntry[]>([])
  let totalCount = $state(0)
  let currentPage = $state(0)
  let pageSize = $state(50)
  let loading = $state(true)
  let stats = $state<LogStats | null>(null)

  // Filters
  let levelFilter = $state<string | null>(null)
  let categoryFilter = $state<string | null>(null)
  let search = $state("")
  let searchTimeout: ReturnType<typeof setTimeout>

  // Live mode
  let liveMode = $state(true)
  let liveInterval: ReturnType<typeof setInterval> | null = null
  let unlisten: (() => void) | null = null
  let logContainer: HTMLDivElement | undefined = $state()

  let totalPages = $derived(Math.ceil(totalCount / pageSize))

  const levels = ["ERROR", "WARN", "INFO", "DEBUG"]
  const categories = ["app", "download", "metadata", "settings", "dependency"]

  onMount(async () => {
    await Promise.all([loadLogs(), loadStats()])

    // Listen for new log events (live mode)
    try {
      const unlistenFn = await listen("new-log-event", (event: any) => {
        if (!liveMode) return
        const entry = event.payload.entry as LogEntry

        // Apply current filters
        if (levelFilter && entry.level !== levelFilter) return
        if (categoryFilter && entry.category !== categoryFilter) return
        if (search && !entry.message.toLowerCase().includes(search.toLowerCase())) return

        logs = [entry, ...logs].slice(0, 200) // Keep max 200 in live mode
        totalCount += 1
        loadStats() // refresh stats
      })
      unlisten = unlistenFn
    } catch (e) {
      console.error("Failed to listen for log events:", e)
    }
  })

  onDestroy(() => {
    if (liveInterval) clearInterval(liveInterval)
    if (unlisten) unlisten()
    clearTimeout(searchTimeout)
  })

  async function loadLogs() {
    loading = true
    try {
      const result = await commands.getLogs(
        currentPage,
        pageSize,
        levelFilter ?? null,
        categoryFilter ?? null,
        search || null,
        null,
      )
      if (result.status === "ok") {
        logs = result.data.items
        totalCount = result.data.totalCount
      }
    } catch (e) {
      console.error("Failed to load logs:", e)
    } finally {
      loading = false
    }
  }

  async function loadStats() {
    try {
      const result = await commands.getLogStats()
      if (result.status === "ok") {
        stats = result.data
      }
    } catch (e) {
      console.error("Failed to load stats:", e)
    }
  }

  function handleSearch(value: string) {
    clearTimeout(searchTimeout)
    search = value
    searchTimeout = setTimeout(() => {
      currentPage = 0
      loadLogs()
    }, 300)
  }

  function setLevel(level: string | null) {
    levelFilter = level
    currentPage = 0
    loadLogs()
  }

  function setCategory(cat: string | null) {
    categoryFilter = cat
    currentPage = 0
    loadLogs()
  }

  function toggleLive() {
    liveMode = !liveMode
    if (!liveMode) {
      currentPage = 0
      loadLogs()
    }
  }

  async function handleClear() {
    if (!confirm(t("logs.clearConfirm"))) return
    try {
      const result = await commands.clearLogs(null)
      if (result.status === "ok") {
        logs = []
        totalCount = 0
        await loadStats()
      }
    } catch (e) {
      console.error("Failed to clear logs:", e)
    }
  }

  function goToPage(page: number) {
    currentPage = page
    loadLogs()
  }

  function formatTimestamp(ts: number): string {
    const date = new Date(ts)
    const h = date.getHours().toString().padStart(2, "0")
    const m = date.getMinutes().toString().padStart(2, "0")
    const s = date.getSeconds().toString().padStart(2, "0")
    const ms = date.getMilliseconds().toString().padStart(3, "0")
    return `${h}:${m}:${s}.${ms}`
  }

  function levelColor(level: string): string {
    switch (level) {
      case "ERROR": return "text-red-400"
      case "WARN": return "text-yellow-400"
      case "INFO": return "text-blue-400"
      case "DEBUG": return "text-gray-400"
      default: return "text-yt-text-secondary"
    }
  }

  function levelBadgeColor(level: string): string {
    switch (level) {
      case "ERROR": return "bg-red-500/20 text-red-400 ring-red-500/30"
      case "WARN": return "bg-yellow-500/20 text-yellow-400 ring-yellow-500/30"
      case "INFO": return "bg-blue-500/20 text-blue-400 ring-blue-500/30"
      default: return "bg-yt-highlight text-yt-text-secondary ring-yt-border"
    }
  }
</script>

<div class="h-full flex flex-col overflow-hidden">
  <!-- Header -->
  <div class="px-6 pt-2 pb-4 border-b border-yt-border bg-yt-bg">
    <div class="flex items-center justify-between mb-4">
      <div>
        <h2 class="font-display text-lg font-semibold text-yt-text">{t("logs.title")}</h2>
        <p class="text-xs text-yt-text-secondary mt-0.5">{t("logs.subtitle")}</p>
      </div>
      <div class="flex items-center gap-2">
        <!-- Stats Badges -->
        {#if stats}
          <div class="flex items-center gap-1.5">
            {#if stats.errorCount > 0}
              <span class="text-[10px] font-mono font-semibold px-2 py-0.5 rounded-full ring-1 {levelBadgeColor('ERROR')}">
                {stats.errorCount} ERR
              </span>
            {/if}
            {#if stats.warnCount > 0}
              <span class="text-[10px] font-mono font-semibold px-2 py-0.5 rounded-full ring-1 {levelBadgeColor('WARN')}">
                {stats.warnCount} WARN
              </span>
            {/if}
            <span class="text-[10px] font-mono font-semibold px-2 py-0.5 rounded-full ring-1 {levelBadgeColor('INFO')}">
              {stats.totalCount} total
            </span>
          </div>
        {/if}

        <!-- Live Toggle -->
        <button
          onclick={toggleLive}
          class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium transition-colors
            {liveMode
              ? 'bg-green-500/20 text-green-400 ring-1 ring-green-500/30'
              : 'bg-yt-highlight text-yt-text-secondary ring-1 ring-yt-border hover:bg-yt-overlay'}"
        >
          {#if liveMode}
            <span class="w-1.5 h-1.5 bg-green-400 rounded-full animate-pulse"></span>
          {/if}
          {t("logs.live")}
        </button>

        <!-- Clear -->
        <button
          onclick={handleClear}
          class="flex items-center gap-1 px-3 py-1.5 rounded-lg text-xs font-medium bg-yt-highlight text-yt-text-secondary ring-1 ring-yt-border hover:bg-red-500/10 hover:text-red-400 hover:ring-red-500/30 transition-colors"
        >
          <span class="material-symbols-outlined text-[14px]">delete</span>
          {t("logs.clearLogs")}
        </button>
      </div>
    </div>

    <!-- Filter Bar -->
    <div class="flex items-center gap-3">
      <!-- Level Filter Chips -->
      <div class="flex items-center gap-1">
        <button
          onclick={() => setLevel(null)}
          class="px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors
            {levelFilter === null
              ? 'bg-yt-primary text-white'
              : 'bg-yt-highlight text-yt-text-secondary hover:bg-yt-overlay'}"
        >
          {t("logs.allLevels")}
        </button>
        {#each levels as level}
          <button
            onclick={() => setLevel(level)}
            class="px-2.5 py-1 rounded-md text-[11px] font-medium transition-colors
              {levelFilter === level
                ? 'bg-yt-primary text-white'
                : 'bg-yt-highlight text-yt-text-secondary hover:bg-yt-overlay'}"
          >
            {level}
          </button>
        {/each}
      </div>

      <!-- Category Dropdown -->
      <select
        value={categoryFilter ?? ""}
        onchange={(e) => setCategory((e.target as HTMLSelectElement).value || null)}
        class="bg-yt-highlight border border-yt-border rounded-md px-2.5 py-1 text-[11px] text-yt-text focus:outline-none focus:ring-1 focus:ring-yt-primary"
      >
        <option value="">{t("logs.allCategories")}</option>
        {#each categories as cat}
          <option value={cat}>{cat}</option>
        {/each}
      </select>

      <!-- Search -->
      <div class="flex-1 relative">
        <span class="material-symbols-outlined absolute left-2.5 top-1/2 -translate-y-1/2 text-yt-text-muted text-[16px]">search</span>
        <input
          type="text"
          value={search}
          oninput={(e) => handleSearch((e.target as HTMLInputElement).value)}
          placeholder={t("logs.searchPlaceholder")}
          class="w-full bg-yt-highlight border border-yt-border rounded-md pl-8 pr-3 py-1.5 text-xs text-yt-text placeholder-yt-text-muted focus:outline-none focus:ring-1 focus:ring-yt-primary"
        />
      </div>
    </div>
  </div>

  <!-- Log List -->
  <div class="flex-1 overflow-y-auto" bind:this={logContainer}>
    {#if loading && logs.length === 0}
      <div class="flex items-center justify-center py-20">
        <span class="material-symbols-outlined text-yt-primary text-3xl animate-spin">progress_activity</span>
      </div>
    {:else if logs.length === 0}
      <div class="flex flex-col items-center justify-center py-20 text-yt-text-muted">
        <span class="material-symbols-outlined text-4xl mb-2 opacity-40">description</span>
        <p class="text-sm">{t("logs.empty")}</p>
      </div>
    {:else}
      <div class="font-mono text-[12px] leading-relaxed">
        {#each logs as entry (entry.id)}
          <div class="flex items-start gap-0 px-4 py-1 hover:bg-yt-highlight/50 transition-colors border-b border-yt-border/30 group">
            <!-- Timestamp -->
            <span class="text-yt-text-muted shrink-0 w-[90px]">{formatTimestamp(entry.timestamp)}</span>

            <!-- Level -->
            <span class="shrink-0 w-[52px] font-semibold {levelColor(entry.level)}">{entry.level.padEnd(5)}</span>

            <!-- Category -->
            <span class="shrink-0 w-[90px] text-yt-text-secondary">[{entry.category}]</span>

            <!-- Message -->
            <span class="text-yt-text break-all flex-1 min-w-0">{entry.message}</span>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Pagination (only when not in live mode) -->
  {#if !liveMode && totalPages > 1}
    <div class="px-6 py-3 border-t border-yt-border bg-yt-bg flex items-center justify-between">
      <p class="text-xs text-yt-text-secondary">
        {totalCount} logs
      </p>
      <div class="flex items-center gap-1">
        <button
          onclick={() => goToPage(currentPage - 1)}
          disabled={currentPage === 0}
          class="p-1 rounded hover:bg-yt-highlight disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
        >
          <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">chevron_left</span>
        </button>
        <span class="text-xs text-yt-text-secondary px-2">
          {currentPage + 1} / {totalPages}
        </span>
        <button
          onclick={() => goToPage(currentPage + 1)}
          disabled={currentPage >= totalPages - 1}
          class="p-1 rounded hover:bg-yt-highlight disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
        >
          <span class="material-symbols-outlined text-[18px] text-yt-text-secondary">chevron_right</span>
        </button>
      </div>
    </div>
  {/if}
</div>
