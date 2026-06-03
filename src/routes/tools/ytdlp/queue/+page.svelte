<script lang="ts">
  import { commands } from "$lib/bindings"
  import type { HistoryItem } from "$lib/bindings"
  import { onMount, onDestroy } from "svelte"
  import { t, getDateLocale } from "$lib/i18n/index.svelte"
  import { formatSize } from "$lib/utils/format"
  import { fade } from "svelte/transition"
  import GroupHeader from "$lib/components/GroupHeader.svelte"

  // "In progress" section — downloads table, everything not yet completed.
  let active = $state<any[]>([])
  // "Records" section — history (completed). Now a HistoryEntry[] (group | single).
  let history = $state<any[]>([])
  let historyTotal = $state(0)
  let page = $state(0)
  let pageSize = $state(20)
  let search = $state("")
  let firstLoad = $state(true)
  let expandedErrors = $state<Set<number>>(new Set())
  let searchTimeout: ReturnType<typeof setTimeout>
  let activeFilter = $state<string | null>(null)

  // Batch-group fold-out state. Active groups default expanded (you're watching them
  // download); history groups default collapsed. Both survive the 2s poll since they
  // live in their own $state.
  let collapsedActiveGroups = $state<Set<number>>(new Set())
  let expandedHistoryGroups = $state<Set<number>>(new Set())
  let historyGroupItems = $state<Map<number, HistoryItem[]>>(new Map())

  let totalPages = $derived(Math.ceil(historyTotal / pageSize))
  let runningCount = $derived(active.filter(i => i.status === "downloading" || i.status === "pending").length)
  let inProgress = $derived(active.filter(i => i.status !== "completed"))
  let filteredActive = $derived(activeFilter ? active.filter(i => i.status === activeFilter) : inProgress)
  // Collapse the active list into group rows (by group_id) + standalone rows.
  let activeRows = $derived(buildActiveRows(filteredActive))
  let activeFilters = $derived([
    { key: null, labelKey: "queue.all", count: inProgress.length },
    { key: "downloading", labelKey: "queue.downloading", count: active.filter(i => i.status === "downloading").length },
    { key: "pending", labelKey: "queue.pending", count: active.filter(i => i.status === "pending").length },
    { key: "failed", labelKey: "queue.failed", count: active.filter(i => i.status === "failed").length },
    { key: "cancelled", labelKey: "queue.cancelled", count: active.filter(i => i.status === "cancelled").length },
    { key: "completed", labelKey: "queue.completed", count: active.filter(i => i.status === "completed").length },
  ])
  let activeTitleKey = $derived(activeFilters.find(f => f.key === activeFilter)?.labelKey ?? "queue.all")

  function buildActiveRows(items: any[]) {
    const rows: any[] = []
    const groups = new Map<number, any>()
    for (const item of items) {
      if (item.groupId != null) {
        let g = groups.get(item.groupId)
        if (!g) {
          g = { kind: "group", groupId: item.groupId, title: item.groupTitle || "—", items: [] }
          groups.set(item.groupId, g)
          rows.push(g)
        }
        g.items.push(item)
      } else {
        rows.push({ kind: "single", item })
      }
    }
    return rows
  }

  function groupDone(items: any[]): number {
    return items.filter(i => i.status === "completed").length
  }
  function groupProgress(items: any[]): number {
    if (!items.length) return 0
    const sum = items.reduce((s, i) => s + (i.status === "completed" ? 100 : (i.progress || 0)), 0)
    return Math.round(sum / items.length)
  }
  function isActiveExpanded(gid: number): boolean {
    return !collapsedActiveGroups.has(gid)
  }
  function toggleActiveGroup(gid: number) {
    const next = new Set(collapsedActiveGroups)
    if (next.has(gid)) next.delete(gid)
    else next.add(gid)
    collapsedActiveGroups = next
  }
  function isHistoryExpanded(gid: number): boolean {
    return expandedHistoryGroups.has(gid)
  }
  function toggleHistoryGroup(gid: number) {
    const next = new Set(expandedHistoryGroups)
    if (next.has(gid)) next.delete(gid)
    else { next.add(gid); loadHistoryGroupItems(gid) }
    expandedHistoryGroups = next
  }
  async function loadHistoryGroupItems(gid: number) {
    try {
      const r = await commands.getGroupHistoryItems(gid)
      if (r.status === "ok") {
        historyGroupItems.set(gid, r.data)
        historyGroupItems = new Map(historyGroupItems)
      }
    } catch (e) { console.error("Failed to load group history items:", e) }
  }

  let interval: ReturnType<typeof setInterval>
  onMount(async () => {
    await Promise.all([loadActive(), loadHistory()])
    firstLoad = false
    interval = setInterval(async () => {
      await loadActive()
      if (page === 0 && !search) await loadHistory()
    }, 2000)
  })

  onDestroy(() => {
    if (interval) clearInterval(interval)
    clearTimeout(searchTimeout)
  })

  async function loadActive() {
    try {
      const r = await commands.getActiveQueue()
      if (r.status === "ok") active = r.data
    } catch (e) { console.error("Failed to load active queue:", e) }
  }

  async function loadHistory() {
    try {
      const r = await commands.getDownloadHistory(page, pageSize, search || null)
      if (r.status === "ok") {
        history = r.data.items
        historyTotal = r.data.totalCount
        if (history.length === 0 && page > 0 && historyTotal > 0) {
          page = Math.max(0, Math.ceil(historyTotal / pageSize) - 1)
          return loadHistory()
        }
        // Keep items of any expanded history group fresh.
        for (const entry of history) {
          if (entry.kind === "group" && expandedHistoryGroups.has(entry.group.groupId)) {
            loadHistoryGroupItems(entry.group.groupId)
          }
        }
      }
    } catch (e) { console.error("Failed to load history:", e) }
  }

  async function handleCancel(id: number) {
    try {
      const r = await commands.cancelDownload(id)
      if (r.status === "ok") await loadActive()
    } catch (e) { console.error("Failed to cancel:", e) }
  }

  async function handleRetry(id: number) {
    try {
      const r = await commands.retryDownload(id)
      if (r.status === "ok") await loadActive()
    } catch (e) { console.error("Failed to retry:", e) }
  }

  async function handleCancelAll() {
    try {
      const r = await commands.cancelAllDownloads()
      if (r.status === "ok") await loadActive()
    } catch (e) { console.error("Failed to cancel all:", e) }
  }

  async function handleCancelGroup(gid: number) {
    try {
      const r = await commands.cancelGroup(gid)
      if (r.status === "ok") await loadActive()
    } catch (e) { console.error("Failed to cancel group:", e) }
  }

  async function handleClearAll() {
    if (!confirm(t("queue.clearAllConfirm"))) return
    try {
      const r = await commands.clearAllQueueAndHistory()
      if (r.status === "ok") {
        activeFilter = null
        await Promise.all([loadActive(), loadHistory()])
      }
    } catch (e) { console.error("Failed to clear all:", e) }
  }

  async function handleDeleteHistory(id: number) {
    if (!confirm(t("history.deleteConfirm"))) return
    try {
      const r = await commands.deleteHistoryItem(id)
      if (r.status === "ok") await loadHistory()
    } catch (e) { console.error("Failed to delete history item:", e) }
  }

  async function handleDeleteHistoryGroup(gid: number) {
    if (!confirm(t("history.deleteGroupConfirm"))) return
    try {
      const r = await commands.deleteHistoryGroup(gid)
      if (r.status === "ok") await loadHistory()
    } catch (e) { console.error("Failed to delete history group:", e) }
  }

  function handleSearch(value: string) {
    clearTimeout(searchTimeout)
    search = value
    searchTimeout = setTimeout(() => { page = 0; loadHistory() }, 300)
  }

  function toggleError(id: number) {
    const next = new Set(expandedErrors)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    expandedErrors = next
  }

  function goToPage(p: number) {
    if (p < 0 || p >= totalPages) return
    page = p
    loadHistory()
  }

  function thumbUrl(videoId: string): string {
    return `https://i.ytimg.com/vi/${videoId}/mqdefault.jpg`
  }

  function hideThumb(e: Event) {
    (e.currentTarget as HTMLImageElement).remove()
  }

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleString(getDateLocale(), { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })
  }
</script>

{#snippet activeItemCard(item: any)}
  <div class="flex items-center gap-3 px-3 py-2.5 rounded-lg bg-yt-surface border border-yt-border/60" in:fade>
    <div class="relative w-16 h-10 rounded overflow-hidden bg-yt-overlay-subtle shrink-0">
      <span class="absolute inset-0 flex items-center justify-center material-symbols-outlined text-yt-text-muted text-[18px]">movie</span>
      <img src={thumbUrl(item.videoId)} alt="" loading="lazy" class="absolute inset-0 w-full h-full object-cover" onerror={hideThumb} />
      {#if item.status === "downloading"}
        <div class="absolute inset-0 bg-black/40 flex items-center justify-center">
          <span class="material-symbols-outlined text-white text-[16px] animate-spin">progress_activity</span>
        </div>
      {/if}
    </div>

    <div class="flex-1 min-w-0">
      <div class="flex items-center justify-between gap-2 mb-0.5">
        <h4 class="font-medium text-yt-text text-sm truncate">{item.title}</h4>
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-yt-overlay border border-yt-border text-yt-text-secondary whitespace-nowrap shrink-0">{item.qualityLabel || "N/A"}</span>
      </div>
      <div class="flex items-center gap-3 text-xs">
        {#if item.status === "downloading"}
          <span class="text-yt-primary font-mono">{item.speed || "0 KiB/s"}</span>
          <span class="text-yt-text-muted">ETA {item.eta || "--:--"}</span>
          <div class="flex-1 max-w-32 bg-yt-overlay rounded-full h-1 overflow-hidden">
            <div class="bg-yt-primary h-full transition-all duration-300" style="width: {item.progress || 0}%"></div>
          </div>
        {:else if item.status === "pending"}
          <span class="text-yt-text-secondary">{t("queue.pendingStatus")}</span>
        {:else if item.status === "failed"}
          <button class="text-yt-error hover:underline flex items-center gap-1" onclick={() => toggleError(item.id)}>
            {item.errorMessage ? t(item.errorMessage) : t("queue.failed")}
            <span class="material-symbols-outlined text-[14px]">expand_more</span>
          </button>
        {:else if item.status === "cancelled"}
          <span class="text-yt-text-muted">{t("queue.cancelled")}</span>
        {:else if item.status === "completed"}
          <span class="text-yt-success flex items-center gap-1">
            <span class="material-symbols-outlined text-[14px]">check_circle</span>
            {t("queue.completed")}
          </span>
        {/if}
      </div>
      {#if item.status === "failed" && (item.errorDetail || item.errorMessage) && expandedErrors.has(item.id)}
        <div class="mt-2 text-xs text-yt-error bg-yt-error/5 p-2 rounded border border-yt-error/10 font-mono whitespace-pre-wrap">{item.errorDetail || t(item.errorMessage)}</div>
      {/if}
    </div>

    <div class="shrink-0">
      {#if item.status === "downloading" || item.status === "pending"}
        <button class="p-1.5 rounded-md hover:bg-yt-error/10 text-yt-text-muted hover:text-yt-error transition-colors" onclick={() => handleCancel(item.id)} title={t("download.cancel")}>
          <span class="material-symbols-outlined text-[18px]">close</span>
        </button>
      {:else if item.status === "failed" || item.status === "cancelled"}
        <button class="p-1.5 rounded-md hover:bg-yt-primary/10 text-yt-text-muted hover:text-yt-primary transition-colors" onclick={() => handleRetry(item.id)} title={t("queue.retry")}>
          <span class="material-symbols-outlined text-[18px]">refresh</span>
        </button>
      {/if}
    </div>
  </div>
{/snippet}

{#snippet historyItemCard(item: any)}
  <div class="group flex items-center gap-3 px-3 py-2.5 rounded-lg hover:bg-yt-highlight/40 transition-colors">
    <div class="relative w-16 h-10 rounded overflow-hidden bg-yt-overlay-subtle shrink-0">
      <span class="absolute inset-0 flex items-center justify-center material-symbols-outlined text-yt-success/60 text-[18px]">check_circle</span>
      <img src={thumbUrl(item.videoId)} alt="" loading="lazy" class="absolute inset-0 w-full h-full object-cover" onerror={hideThumb} />
    </div>
    <div class="flex-1 min-w-0">
      <h4 class="font-medium text-yt-text text-sm truncate mb-0.5">{item.title}</h4>
      <div class="flex items-center gap-2 text-xs text-yt-text-secondary flex-wrap">
        <span class="px-1.5 py-0.5 rounded bg-yt-overlay">{item.qualityLabel || "N/A"}</span>
        <span class="px-1.5 py-0.5 rounded bg-yt-overlay">{item.format}</span>
        <span>{formatSize(item.fileSize, "-")}</span>
        <span>{formatDate(item.downloadedAt)}</span>
      </div>
    </div>
    <button
      class="opacity-0 group-hover:opacity-100 text-yt-text-muted hover:text-yt-error transition-all p-1.5 rounded-md hover:bg-yt-error/10 shrink-0"
      onclick={() => handleDeleteHistory(item.id)}
      aria-label="Delete"
    >
      <span class="material-symbols-outlined text-[18px]">delete</span>
    </button>
  </div>
{/snippet}

<div class="flex-1 flex flex-col h-full bg-yt-bg overflow-y-auto hide-scrollbar">
  <header class="px-6 py-6 shrink-0 border-b border-yt-border bg-yt-surface/30">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-lg font-semibold text-yt-text">{t("nav.queueHistory")}</h2>
        <p class="text-xs text-yt-text-secondary mt-1">{t("queue.subtitle")}</p>
      </div>
      <div class="flex items-center gap-2 shrink-0">
        {#if runningCount > 0}
          <button
            class="px-3 py-1.5 rounded-md bg-yt-warning/10 text-yt-warning hover:bg-yt-warning/20 text-xs font-medium transition-colors"
            onclick={handleCancelAll}
          >
            {t("queue.cancelAll")}
          </button>
        {/if}
        <button
          class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-error/10 hover:text-yt-error border border-yt-border text-yt-text-secondary text-xs font-medium transition-colors"
          onclick={handleClearAll}
        >
          {t("queue.clearAll")}
        </button>
      </div>
    </div>
  </header>

  {#if firstLoad}
    <div class="flex justify-center py-16">
      <span class="material-symbols-outlined text-yt-primary text-3xl animate-spin">progress_activity</span>
    </div>
  {:else}
    <!-- In-progress section -->
    {#if active.length > 0}
      <section class="px-6 pt-5">
        <div class="flex items-center justify-between gap-3 mb-3 flex-wrap">
          <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider">
            {t(activeTitleKey)} · {filteredActive.length}
          </h3>
          <div class="flex gap-1 flex-wrap">
            {#each activeFilters as f}
              <button
                class="px-2 py-0.5 rounded-md text-[11px] font-medium transition-colors {activeFilter === f.key ? 'bg-yt-primary text-white' : 'bg-yt-surface border border-yt-border text-yt-text-secondary hover:bg-yt-highlight'}"
                onclick={() => activeFilter = f.key}
              >
                {t(f.labelKey)} <span class="opacity-60">{f.count}</span>
              </button>
            {/each}
          </div>
        </div>
        <div class="space-y-2">
          {#each activeRows as row (row.kind === "group" ? `g${row.groupId}` : `s${row.item.id}`)}
            {#if row.kind === "group"}
              <GroupHeader
                title={row.title}
                subtitle={t("queue.groupProgress", { completed: groupDone(row.items), total: row.items.length })}
                progress={groupProgress(row.items)}
                expanded={isActiveExpanded(row.groupId)}
                onToggle={() => toggleActiveGroup(row.groupId)}
                onAction={() => handleCancelGroup(row.groupId)}
                actionIcon="close"
                actionTitle={t("queue.cancelGroup")}
              />
              {#if isActiveExpanded(row.groupId)}
                <div class="pl-6 space-y-2">
                  {#each row.items as item (item.id)}
                    {@render activeItemCard(item)}
                  {/each}
                </div>
              {/if}
            {:else}
              {@render activeItemCard(row.item)}
            {/if}
          {/each}
        </div>
      </section>
    {/if}

    <!-- Records (history) section -->
    <section class="px-6 py-5 flex-1">
      <div class="flex items-center justify-between mb-3 gap-3">
        <h3 class="text-xs font-semibold text-yt-text-secondary uppercase tracking-wider">{t("queue.records")}</h3>
        <div class="relative w-56 max-w-full">
          <div class="absolute inset-y-0 left-3 flex items-center pointer-events-none text-yt-text-muted">
            <span class="material-symbols-outlined text-[18px]">search</span>
          </div>
          <input
            type="text"
            class="w-full h-8 bg-yt-surface text-yt-text rounded-lg pl-9 pr-3 border border-yt-border focus:ring-2 focus:ring-yt-primary focus:outline-none text-xs"
            placeholder={t("history.searchPlaceholder")}
            value={search}
            oninput={(e) => handleSearch((e.target as HTMLInputElement).value)}
          />
        </div>
      </div>

      {#if history.length === 0}
        <div class="flex flex-col items-center justify-center py-16 text-center" in:fade>
          <span class="material-symbols-outlined text-yt-border text-5xl mb-2">library_books</span>
          <p class="text-yt-text-secondary text-sm">{search ? t("history.empty") : t("queue.emptyDesc")}</p>
        </div>
      {:else}
        <div class="space-y-2">
          {#each history as entry (entry.kind === "group" ? `g${entry.group.groupId}` : `s${entry.item.id}`)}
            {#if entry.kind === "group"}
              <GroupHeader
                title={entry.group.title}
                subtitle={t("queue.groupProgress", { completed: entry.group.completedCount, total: entry.group.totalCount })}
                expanded={isHistoryExpanded(entry.group.groupId)}
                onToggle={() => toggleHistoryGroup(entry.group.groupId)}
                onAction={() => handleDeleteHistoryGroup(entry.group.groupId)}
                actionIcon="delete"
                actionTitle={t("history.deleteGroup")}
              />
              {#if isHistoryExpanded(entry.group.groupId)}
                <div class="pl-6 space-y-2">
                  {#each (historyGroupItems.get(entry.group.groupId) ?? []) as item (item.id)}
                    {@render historyItemCard(item)}
                  {/each}
                </div>
              {/if}
            {:else}
              {@render historyItemCard(entry.item)}
            {/if}
          {/each}
        </div>

        {#if totalPages > 1}
          <div class="flex items-center justify-center gap-2 pt-4">
            <button class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text text-xs disabled:opacity-50 disabled:cursor-not-allowed" onclick={() => goToPage(page - 1)} disabled={page === 0}>
              <span class="material-symbols-outlined text-[16px]">chevron_left</span>
            </button>
            <span class="text-xs text-yt-text-secondary px-2">{page + 1} / {totalPages}</span>
            <button class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text text-xs disabled:opacity-50 disabled:cursor-not-allowed" onclick={() => goToPage(page + 1)} disabled={page >= totalPages - 1}>
              <span class="material-symbols-outlined text-[16px]">chevron_right</span>
            </button>
          </div>
        {/if}
      {/if}
    </section>
  {/if}
</div>
