<script lang="ts">
  import { commands } from "$lib/bindings"
  import type { DownloadTaskInfo, GlobalDownloadEvent, HistoryEntry, HistoryItem } from "$lib/bindings"
  import { ask } from "@tauri-apps/plugin-dialog"
  import { listen } from "@tauri-apps/api/event"
  import { revealItemInDir, openPath } from "@tauri-apps/plugin-opener"
  import { onMount, onDestroy } from "svelte"
  import { fade } from "svelte/transition"
  import { t } from "$lib/i18n/index.svelte"
  import { errorMessage, extractError } from "$lib/utils/errors"
  import {
    activeRowKey,
    buildActiveFilters,
    buildActiveRows,
    groupDenominator as getGroupDenominator,
    groupDone,
    groupProgress as getGroupProgress,
    historyEntryKey,
    nextGroupMaxCounts,
    type ActiveFilter,
  } from "$lib/ytdlp/queue-view"
  import GroupHeader from "$lib/components/GroupHeader.svelte"
  import QueueActiveItem from "$lib/components/QueueActiveItem.svelte"
  import QueueHistoryItem from "$lib/components/QueueHistoryItem.svelte"

  // "In progress" section — downloads table, everything not yet completed.
  let active = $state<DownloadTaskInfo[]>([])
  // "Records" section — history (completed). Now a HistoryEntry[] (group | single).
  let history = $state<HistoryEntry[]>([])
  let historyTotal = $state(0)
  let page = $state(0)
  let pageSize = $state(20)
  let search = $state("")
  let firstLoad = $state(true)
  let expandedErrors = $state<Set<number>>(new Set())
  let searchTimeout: ReturnType<typeof setTimeout>
  let activeFilter = $state<ActiveFilter>(null)
  let busyActions = $state<Set<string>>(new Set())
  // Last queue/history action failure (retry/cancel/clear/delete), shown as a dismissible banner.
  let actionError = $state<string | null>(null)

  // The 5s poll fails silently on transient errors, but several failures in a
  // row mean the list on screen is stale — surface that as its own banner
  // (separate from actionError so dismissing one doesn't hide the other).
  const POLL_FAIL_THRESHOLD = 3
  let pollFailCount = 0
  let pollError = $state(false)

  function recordPollResult(ok: boolean) {
    if (ok) {
      pollFailCount = 0
      pollError = false
    } else {
      pollFailCount++
      if (pollFailCount >= POLL_FAIL_THRESHOLD) pollError = true
    }
  }

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
  let activeFilters = $derived(buildActiveFilters(active, inProgress))
  let activeTitleKey = $derived(activeFilters.find(f => f.key === activeFilter)?.labelKey ?? "queue.all")

  // The active-queue payload only carries items still in the downloads table, so once a group's
  // videos complete they fall out and the visible item count shrinks. We remember the largest
  // count ever seen per group (this session) and use that as the progress denominator so the
  // bar can't jump backwards as members finish and disappear.
  let groupMaxCount = $state<Map<number, number>>(new Map())

  function groupDenominator(gid: number, visible: number): number {
    return getGroupDenominator(groupMaxCount, gid, visible)
  }
  function groupProgress(gid: number, items: DownloadTaskInfo[]): number {
    return getGroupProgress(groupMaxCount, gid, items)
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
      const requestId = (groupLoadSeq.get(gid) ?? 0) + 1
      groupLoadSeq.set(gid, requestId)
      const r = await commands.getGroupHistoryItems(gid)
      if (requestId !== groupLoadSeq.get(gid)) return
      if (r.status === "ok") {
        historyGroupItems.set(gid, r.data)
        historyGroupItems = new Map(historyGroupItems)
      }
    } catch (e) { console.error("Failed to load group history items:", e) }
  }

  let interval: ReturnType<typeof setInterval>
  let unlistenDownload: (() => void) | null = null
  let activeLoadSeq = 0
  let historyLoadSeq = 0
  let groupLoadSeq = new Map<number, number>()

  function isBusy(key: string): boolean {
    return busyActions.has(key)
  }

  async function withBusy(key: string, action: () => Promise<void>) {
    if (busyActions.has(key)) return
    actionError = null
    busyActions = new Set([...busyActions, key])
    try {
      await action()
    } finally {
      const next = new Set(busyActions)
      next.delete(key)
      busyActions = next
    }
  }

  onMount(async () => {
    await Promise.all([loadActive(), loadHistory()])
    firstLoad = false

    // Drive updates off the global download-event stream: progress events patch the affected
    // row in place (cheap), while status transitions re-pull the active queue (and history,
    // on completion) so rows move between sections. A slow 5s poll stays as a safety net.
    try {
      unlistenDownload = await listen<GlobalDownloadEvent>("download-event", (event) => {
        const data = event.payload
        if (data.eventType === "progress") {
          const idx = active.findIndex(d => d.id === data.taskId)
          if (idx !== -1) {
            active[idx] = {
              ...active[idx],
              progress: data.percent ?? active[idx].progress,
              speed: data.speed ?? active[idx].speed,
              eta: data.eta ?? active[idx].eta,
            }
          }
        } else {
          loadActive()
          if (data.eventType === "completed" && page === 0 && !search) loadHistory()
        }
      })
    } catch (e) { console.error("Failed to listen for download events:", e) }

    interval = setInterval(async () => {
      await loadActive()
      if (page === 0 && !search) await loadHistory()
    }, 5000)
  })

  onDestroy(() => {
    activeLoadSeq++
    historyLoadSeq++
    groupLoadSeq.clear()
    if (interval) clearInterval(interval)
    if (unlistenDownload) unlistenDownload()
    clearTimeout(searchTimeout)
  })

  async function loadActive() {
    const requestId = ++activeLoadSeq
    try {
      const r = await commands.getActiveQueue()
      if (requestId !== activeLoadSeq) return
      if (r.status === "ok") {
        active = r.data
        groupMaxCount = nextGroupMaxCounts(groupMaxCount, r.data)
        recordPollResult(true)
      } else {
        console.error("Failed to load active queue:", r.error)
        recordPollResult(false)
      }
    } catch (e) {
      console.error("Failed to load active queue:", e)
      recordPollResult(false)
    }
  }

  async function loadHistory() {
    const requestId = ++historyLoadSeq
    const requestedPage = page
    const requestedSearch = search
    try {
      const r = await commands.getDownloadHistory(requestedPage, pageSize, requestedSearch || null)
      if (requestId !== historyLoadSeq || requestedPage !== page || requestedSearch !== search) return
      if (r.status === "ok") {
        history = r.data.items
        historyTotal = r.data.totalCount
        recordPollResult(true)
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
      } else {
        console.error("Failed to load history:", r.error)
        recordPollResult(false)
      }
    } catch (e) {
      console.error("Failed to load history:", e)
      recordPollResult(false)
    }
  }

  async function handleCancel(id: number) {
    await withBusy(`cancel:${id}`, async () => {
      try {
        const r = await commands.cancelDownload(id)
        if (r.status === "ok") await loadActive()
        else {
          console.error("Failed to cancel:", r.error)
          actionError = extractError(r.error)
        }
      } catch (e) {
        console.error("Failed to cancel:", e)
        actionError = errorMessage(e)
      }
    })
  }

  async function handleRetry(id: number) {
    await withBusy(`retry:${id}`, async () => {
      try {
        const r = await commands.retryDownload(id)
        if (r.status === "ok") await loadActive()
        else {
          console.error("Failed to retry:", r.error)
          actionError = extractError(r.error)
        }
      } catch (e) {
        console.error("Failed to retry:", e)
        actionError = errorMessage(e)
      }
    })
  }

  async function handleCancelAll() {
    await withBusy("cancel-all", async () => {
      try {
        const r = await commands.cancelAllDownloads()
        if (r.status === "ok") await loadActive()
        else {
          console.error("Failed to cancel all:", r.error)
          actionError = extractError(r.error)
        }
      } catch (e) {
        console.error("Failed to cancel all:", e)
        actionError = errorMessage(e)
      }
    })
  }

  async function handleCancelGroup(gid: number) {
    await withBusy(`cancel-group:${gid}`, async () => {
      try {
        const r = await commands.cancelGroup(gid)
        if (r.status === "ok") await loadActive()
        else {
          console.error("Failed to cancel group:", r.error)
          actionError = extractError(r.error)
        }
      } catch (e) {
        console.error("Failed to cancel group:", e)
        actionError = errorMessage(e)
      }
    })
  }

  async function handleClearAll() {
    if (!(await ask(t("queue.clearAllConfirm"), { kind: "warning" }))) return
    await withBusy("clear-all", async () => {
      try {
        const r = await commands.clearAllQueueAndHistory()
        if (r.status === "ok") {
          activeFilter = null
          await Promise.all([loadActive(), loadHistory()])
        } else {
          console.error("Failed to clear all:", r.error)
          actionError = extractError(r.error)
        }
      } catch (e) {
        console.error("Failed to clear all:", e)
        actionError = errorMessage(e)
      }
    })
  }

  async function handleDeleteHistory(id: number) {
    if (!(await ask(t("history.deleteConfirm"), { kind: "warning" }))) return
    await withBusy(`delete-history:${id}`, async () => {
      try {
        const r = await commands.deleteHistoryItem(id)
        if (r.status === "ok") await loadHistory()
        else {
          console.error("Failed to delete history item:", r.error)
          actionError = extractError(r.error)
        }
      } catch (e) {
        console.error("Failed to delete history item:", e)
        actionError = errorMessage(e)
      }
    })
  }

  async function handleDeleteHistoryGroup(gid: number) {
    if (!(await ask(t("history.deleteGroupConfirm"), { kind: "warning" }))) return
    await withBusy(`delete-history-group:${gid}`, async () => {
      try {
        const r = await commands.deleteHistoryGroup(gid)
        if (r.status === "ok") await loadHistory()
        else {
          console.error("Failed to delete history group:", r.error)
          actionError = extractError(r.error)
        }
      } catch (e) {
        console.error("Failed to delete history group:", e)
        actionError = errorMessage(e)
      }
    })
  }

  async function handleReveal(filePath: string | null | undefined) {
    if (!filePath) return
    try {
      await revealItemInDir(filePath)
    } catch (e) {
      console.error("Failed to reveal file:", e)
      actionError = t("history.revealFailed")
    }
  }

  async function handleOpenFile(filePath: string | null | undefined) {
    if (!filePath) return
    try {
      await openPath(filePath)
    } catch (e) {
      // Typically the file was moved/deleted after download — point at the folder instead.
      console.error("Failed to open file:", e)
      actionError = t("history.openFileFailed")
    }
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

</script>

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
            class="px-3 py-1.5 rounded-md bg-yt-warning/10 text-yt-warning hover:bg-yt-warning/20 text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            onclick={handleCancelAll}
            disabled={isBusy("cancel-all")}
          >
            {t("queue.cancelAll")}
          </button>
        {/if}
        <button
          class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-error/10 hover:text-yt-error border border-yt-border text-yt-text-secondary text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={handleClearAll}
          disabled={isBusy("clear-all")}
        >
          {t("queue.clearAll")}
        </button>
      </div>
    </div>
  </header>

  {#if actionError}
    <div class="mx-6 mt-4 bg-yt-error/10 border border-yt-error/20 rounded-lg px-4 py-3 flex items-start gap-3">
      <span class="material-symbols-outlined text-yt-error text-[20px] shrink-0 mt-0.5">error</span>
      <div class="flex-1 min-w-0">
        <p class="text-sm text-yt-text font-medium">{t("queue.actionFailed")}</p>
        <p class="text-xs text-yt-text-secondary mt-0.5">{actionError}</p>
      </div>
      <button class="text-yt-text-secondary hover:text-yt-text" aria-label={t("download.close")} onclick={() => actionError = null}>
        <span class="material-symbols-outlined text-[18px]">close</span>
      </button>
    </div>
  {/if}

  {#if pollError}
    <div class="mx-6 mt-4 bg-yt-error/10 border border-yt-error/20 rounded-lg px-4 py-3 flex items-start gap-3">
      <span class="material-symbols-outlined text-yt-error text-[20px] shrink-0 mt-0.5">sync_problem</span>
      <div class="flex-1 min-w-0">
        <p class="text-sm text-yt-text font-medium">{t("queue.pollFailed")}</p>
      </div>
      <button class="text-yt-text-secondary hover:text-yt-text" aria-label={t("download.close")} onclick={() => pollError = false}>
        <span class="material-symbols-outlined text-[18px]">close</span>
      </button>
    </div>
  {/if}

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
          {#each activeRows as row (activeRowKey(row))}
            {#if row.kind === "group"}
              <GroupHeader
                title={row.title}
                subtitle={t("queue.groupProgress", { completed: groupDone(row.items), total: groupDenominator(row.groupId, row.items.length) })}
                progress={groupProgress(row.groupId, row.items)}
                expanded={isActiveExpanded(row.groupId)}
                onToggle={() => toggleActiveGroup(row.groupId)}
                onAction={() => handleCancelGroup(row.groupId)}
                actionIcon="close"
                actionTitle={t("queue.cancelGroup")}
                actionDisabled={isBusy(`cancel-group:${row.groupId}`)}
              />
              {#if isActiveExpanded(row.groupId)}
                <div class="pl-6 space-y-2">
                  {#each row.items as item (item.id)}
                    <QueueActiveItem
                      {item}
                      errorExpanded={expandedErrors.has(item.id)}
                      cancelBusy={isBusy(`cancel:${item.id}`)}
                      retryBusy={isBusy(`retry:${item.id}`)}
                      onCancel={() => handleCancel(item.id)}
                      onRetry={() => handleRetry(item.id)}
                      onToggleError={() => toggleError(item.id)}
                    />
                  {/each}
                </div>
              {/if}
            {:else}
              <QueueActiveItem
                item={row.item}
                errorExpanded={expandedErrors.has(row.item.id)}
                cancelBusy={isBusy(`cancel:${row.item.id}`)}
                retryBusy={isBusy(`retry:${row.item.id}`)}
                onCancel={() => handleCancel(row.item.id)}
                onRetry={() => handleRetry(row.item.id)}
                onToggleError={() => toggleError(row.item.id)}
              />
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
          {#each history as entry (historyEntryKey(entry))}
            {#if entry.kind === "group"}
              <GroupHeader
                title={entry.group.title}
                subtitle={t("queue.groupProgress", { completed: entry.group.completedCount, total: entry.group.totalCount })}
                expanded={isHistoryExpanded(entry.group.groupId)}
                onToggle={() => toggleHistoryGroup(entry.group.groupId)}
                onAction={() => handleDeleteHistoryGroup(entry.group.groupId)}
                actionIcon="delete"
                actionTitle={t("history.deleteGroup")}
                actionDisabled={isBusy(`delete-history-group:${entry.group.groupId}`)}
              />
              {#if isHistoryExpanded(entry.group.groupId)}
                <div class="pl-6 space-y-2">
                  {#each (historyGroupItems.get(entry.group.groupId) ?? []) as item (item.id)}
                    <QueueHistoryItem
                      {item}
                      deleteBusy={isBusy(`delete-history:${item.id}`)}
                      onOpen={() => handleOpenFile(item.filePath)}
                      onReveal={() => handleReveal(item.filePath)}
                      onDelete={() => handleDeleteHistory(item.id)}
                    />
                  {/each}
                </div>
              {/if}
            {:else}
              <QueueHistoryItem
                item={entry.item}
                deleteBusy={isBusy(`delete-history:${entry.item.id}`)}
                onOpen={() => handleOpenFile(entry.item.filePath)}
                onReveal={() => handleReveal(entry.item.filePath)}
                onDelete={() => handleDeleteHistory(entry.item.id)}
              />
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
