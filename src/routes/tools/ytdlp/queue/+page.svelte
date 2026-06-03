<script lang="ts">
  import { commands } from "$lib/bindings"
  import type { DownloadTaskInfo } from "$lib/bindings"
  import { onMount, onDestroy } from "svelte"
  import { listen } from "@tauri-apps/api/event"
  import { t } from "$lib/i18n/index.svelte"
  import { fade } from "svelte/transition"
  import GroupHeader from "$lib/components/GroupHeader.svelte"

  // Each entry is a QueueEntry: { kind: "group", group } | { kind: "single", item }
  let queue = $state<any[]>([])
  let totalCount = $state(0)
  let currentPage = $state(0)
  let pageSize = $state(20)
  let statusFilter = $state<string | null>(null)
  let firstLoad = $state(true)
  let expandedErrors = $state<Set<number>>(new Set())

  // Group fold-out state. Kept in separate $state so the 2s poll (which replaces
  // `queue` wholesale) never resets what the user has expanded.
  let expandedGroups = $state<Set<number>>(new Set())
  let userTouchedGroups = $state<Set<number>>(new Set())
  let groupItems = $state<Map<number, DownloadTaskInfo[]>>(new Map())

  // Server-side counts
  let activeCount = $state(0)
  let pendingCount = $state(0)
  let completedCount = $state(0)
  let failedCount = $state(0)
  let cancelledCount = $state(0)

  let totalPages = $derived(Math.ceil(totalCount / pageSize))

  let interval: ReturnType<typeof setInterval>
  let unlisten: (() => void) | null = null
  onMount(async () => {
    await loadQueue()
    firstLoad = false
    interval = setInterval(loadQueue, 2000)
    // Smooth per-item progress between polls; status changes trigger an authoritative reload.
    unlisten = await listen("download-event", (event: any) => {
      const data = event.payload
      if (data.eventType === "progress") {
        patchItemProgress(data.taskId, data.percent ?? 0, data.speed, data.eta)
      } else {
        loadQueue()
      }
    })
  })

  onDestroy(() => {
    if (interval) clearInterval(interval)
    if (unlisten) unlisten()
  })

  async function loadQueue() {
    try {
      const result = await commands.getDownloadQueuePaginated(currentPage, pageSize, statusFilter)
      if (result.status === "ok") {
        const data = result.data
        queue = data.items
        totalCount = data.totalCount
        activeCount = data.activeCount
        pendingCount = data.pendingCount
        completedCount = data.completedCount
        failedCount = data.failedCount
        cancelledCount = data.cancelledCount
        // Refresh items of any group that is currently expanded.
        for (const entry of queue) {
          if (entry.kind === "group" && isExpanded(entry.group)) {
            loadGroupItems(entry.group.groupId)
          }
        }
      }
    } catch (e) {
      console.error("Failed to load queue:", e)
    }
  }

  async function loadGroupItems(groupId: number) {
    try {
      const r = await commands.getGroupQueueItems(groupId)
      if (r.status === "ok") {
        groupItems.set(groupId, r.data)
        groupItems = new Map(groupItems)
      }
    } catch (e) {
      console.error("Failed to load group items:", e)
    }
  }

  // Auto policy when the user hasn't touched a group: expand while in progress,
  // collapse once everything is done. A manual toggle pins the choice afterwards.
  function isExpanded(group: any): boolean {
    if (userTouchedGroups.has(group.groupId)) return expandedGroups.has(group.groupId)
    return group.completedCount < group.totalCount
  }

  function toggleGroup(group: any) {
    const id = group.groupId
    const open = isExpanded(group)
    const next = new Set(expandedGroups)
    if (open) next.delete(id)
    else { next.add(id); loadGroupItems(id) }
    expandedGroups = next
    userTouchedGroups = new Set(userTouchedGroups).add(id)
  }

  function patchItemProgress(taskId: number, percent: number, speed: string | null, eta: string | null) {
    for (const entry of queue) {
      if (entry.kind === "single" && entry.item.id === taskId) {
        entry.item.progress = percent
        entry.item.speed = speed
        entry.item.eta = eta
      }
    }
    for (const [gid, items] of groupItems) {
      const it = items.find((i) => i.id === taskId)
      if (it) {
        it.progress = percent
        it.speed = speed
        it.eta = eta
        const entry = queue.find((e) => e.kind === "group" && e.group.groupId === gid)
        if (entry) {
          const sum = items.reduce((s, i) => s + (i.status === "completed" ? 100 : (i.progress || 0)), 0)
          entry.group.progress = Math.round(sum / Math.max(1, entry.group.totalCount))
        }
      }
    }
    queue = [...queue]
    groupItems = new Map(groupItems)
  }

  async function handleClearCompleted() {
    try {
      const result = await commands.clearCompleted()
      if (result.status === "ok") await loadQueue()
    } catch (e) {
      console.error("Failed to clear completed:", e)
    }
  }

  async function handleCancel(id: number) {
    try {
      const result = await commands.cancelDownload(id)
      if (result.status === "ok") await loadQueue()
      else console.error("Failed to cancel download:", Object.values(result.error)[0])
    } catch (e) {
      console.error("Failed to cancel download:", e)
    }
  }

  async function handleCancelAll() {
    try {
      const result = await commands.cancelAllDownloads()
      if (result.status === "ok") await loadQueue()
    } catch (e) {
      console.error("Failed to cancel all downloads:", e)
    }
  }

  async function handleCancelGroup(groupId: number) {
    try {
      const result = await commands.cancelGroup(groupId)
      if (result.status === "ok") await loadQueue()
    } catch (e) {
      console.error("Failed to cancel group:", e)
    }
  }

  function toggleError(id: number) {
    const next = new Set(expandedErrors)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    expandedErrors = next
  }

  function setFilter(filter: string | null) {
    statusFilter = filter
    currentPage = 0
    loadQueue()
  }

  function goToPage(page: number) {
    if (page < 0 || page >= totalPages) return
    currentPage = page
    loadQueue()
  }

  const statusFilters = [
    { key: null, labelKey: "queue.total", countFn: () => activeCount + pendingCount + completedCount + failedCount + cancelledCount },
    { key: "downloading", labelKey: "queue.active", countFn: () => activeCount, color: "bg-yt-primary" },
    { key: "pending", labelKey: "queue.pendingStatus", countFn: () => pendingCount, color: "bg-yellow-500" },
    { key: "completed", labelKey: "queue.completed", countFn: () => completedCount, color: "bg-yt-success" },
    { key: "failed", labelKey: "queue.failed", countFn: () => failedCount, color: "bg-yt-error" },
  ]
</script>

{#snippet queueItemCard(item: any, indented: boolean)}
  <div class="group flex items-center gap-4 {indented ? 'pl-16 pr-6' : 'px-6'} py-3 hover:bg-yt-highlight/30 transition-colors">
    <!-- Icon/Status -->
    <div class="shrink-0">
      {#if item.status === "downloading"}
        <div class="w-8 h-8 rounded-full bg-yt-primary/10 flex items-center justify-center">
          <span class="material-symbols-outlined text-yt-primary text-[18px] animate-spin">progress_activity</span>
        </div>
      {:else if item.status === "completed"}
        <div class="w-8 h-8 rounded-full bg-yt-success/10 flex items-center justify-center">
          <span class="material-symbols-outlined text-yt-success text-[18px]">check</span>
        </div>
      {:else if item.status === "failed"}
        <div class="w-8 h-8 rounded-full bg-yt-error/10 flex items-center justify-center">
          <span class="material-symbols-outlined text-yt-error text-[18px]">error</span>
        </div>
      {:else}
        <div class="w-8 h-8 rounded-full bg-yt-surface border border-yt-border flex items-center justify-center">
          <span class="material-symbols-outlined text-yt-text-muted text-[18px]">hourglass_empty</span>
        </div>
      {/if}
    </div>

    <!-- Info -->
    <div class="flex-1 min-w-0">
      <div class="flex items-center justify-between mb-1">
        <h4 class="font-medium text-yt-text text-sm truncate pr-4">{item.title}</h4>
        <span class="text-[10px] px-1.5 py-0.5 rounded bg-yt-surface border border-yt-border text-yt-text-secondary whitespace-nowrap">{item.qualityLabel || "N/A"}</span>
      </div>

      <div class="flex items-center justify-between">
        <div class="flex items-center gap-3 text-xs text-yt-text-secondary">
          {#if item.status === "downloading"}
            <span class="text-yt-primary font-mono">{item.speed || "0 KiB/s"}</span>
            <span class="text-yt-text-muted">ETA: {item.eta || "--:--"}</span>
          {:else if item.status === "completed"}
            <span class="text-yt-success">{t("queue.downloadComplete")}</span>
          {:else if item.status === "failed"}
            <button class="text-yt-error hover:underline flex items-center gap-1" onclick={() => toggleError(item.id)}>
              {item.errorMessage ? item.errorMessage.split("\n")[0] : t("queue.failed")}
              <span class="material-symbols-outlined text-[14px]">expand_more</span>
            </button>
          {:else}
            <span>{t("queue.pendingStatus")}</span>
          {/if}
        </div>

        {#if item.status === "downloading"}
          <div class="w-32 bg-yt-surface rounded-full h-1.5 border border-yt-border/50 overflow-hidden relative">
            <div class="bg-yt-primary h-full transition-all duration-300 relative overflow-hidden" style="width: {item.progress || 0}%">
              <div class="absolute inset-0 animate-shimmer"></div>
            </div>
          </div>
        {/if}
      </div>

      {#if item.status === "failed" && item.errorMessage && expandedErrors.has(item.id)}
        <div class="mt-2 text-xs text-yt-error bg-yt-error/5 p-2 rounded border border-yt-error/10 font-mono whitespace-pre-wrap">
          {item.errorMessage}
        </div>
      {/if}
    </div>

    <!-- Actions -->
    <div class="shrink-0 pl-2">
      {#if item.status === "downloading" || item.status === "pending"}
        <button
          class="p-1.5 rounded-md hover:bg-yt-error/10 text-yt-text-muted hover:text-yt-error transition-colors"
          onclick={() => handleCancel(item.id)}
          title="Cancel"
        >
          <span class="material-symbols-outlined text-[18px]">close</span>
        </button>
      {/if}
    </div>
  </div>
{/snippet}

<div class="flex-1 flex flex-col h-full bg-yt-bg">
  <header class="px-6 py-6 shrink-0 border-b border-yt-border bg-yt-surface/30">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-lg font-semibold text-yt-text">{t("queue.title")}</h2>
        <p class="text-xs text-yt-text-secondary mt-1">{t("queue.subtitle")}</p>
      </div>
      <div class="flex gap-2">
        <button
          class="px-3 py-1.5 rounded-md bg-yt-warning/10 text-yt-warning hover:bg-yt-warning/20 text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={handleCancelAll}
          disabled={activeCount + pendingCount === 0}
        >
          {t("queue.cancelAll")}
        </button>
        <button
          class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={handleClearCompleted}
          disabled={completedCount === 0}
        >
          {t("queue.clearCompleted")}
        </button>
      </div>
    </div>

    <!-- Stats -->
    <div class="flex gap-6 mt-4">
      <div class="flex items-center gap-2">
        <span class="w-2 h-2 rounded-full bg-yt-primary"></span>
        <span class="text-xs font-medium text-yt-text">{activeCount}</span>
        <span class="text-xs text-yt-text-secondary">{t("queue.active")}</span>
      </div>
      <div class="flex items-center gap-2">
         <span class="w-2 h-2 rounded-full bg-yt-success"></span>
        <span class="text-xs font-medium text-yt-text">{completedCount}</span>
        <span class="text-xs text-yt-text-secondary">{t("queue.completed")}</span>
      </div>
      <div class="flex items-center gap-2">
         <span class="w-2 h-2 rounded-full bg-yt-text-muted"></span>
        <span class="text-xs font-medium text-yt-text">{totalCount}</span>
        <span class="text-xs text-yt-text-secondary">{t("queue.total")}</span>
      </div>
    </div>

    <!-- Status Filter Tabs -->
    <div class="flex gap-1 mt-4">
      {#each statusFilters as filter}
        <button
          class="px-3 py-1.5 rounded-md text-xs font-medium transition-colors {statusFilter === filter.key ? 'bg-yt-primary text-white' : 'bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text-secondary'}"
          onclick={() => setFilter(filter.key)}
        >
          {t(filter.labelKey)}
          <span class="ml-1 opacity-70">{filter.countFn()}</span>
        </button>
      {/each}
    </div>
  </header>

  <div class="flex-1 overflow-y-auto">
    {#if firstLoad}
      <div class="flex justify-center py-16">
        <span class="material-symbols-outlined text-yt-primary text-3xl animate-spin">progress_activity</span>
      </div>
    {:else if queue.length === 0}
      <div class="flex flex-col items-center justify-center h-64 text-center" in:fade>
        <span class="material-symbols-outlined text-yt-border text-5xl mb-2 animate-float">inbox</span>
        <p class="text-yt-text-secondary text-sm">{t("queue.emptyDesc")}</p>
      </div>
    {:else}
      <div class="divide-y divide-yt-border/50">
        {#each queue as entry (entry.kind === "group" ? `g${entry.group.groupId}` : `s${entry.item.id}`)}
          {#if entry.kind === "group"}
            <GroupHeader
              title={entry.group.title}
              completedCount={entry.group.completedCount}
              totalCount={entry.group.totalCount}
              progress={entry.group.progress}
              expanded={isExpanded(entry.group)}
              variant="queue"
              onToggle={() => toggleGroup(entry.group)}
              onAction={entry.group.activeCount + entry.group.pendingCount > 0 ? () => handleCancelGroup(entry.group.groupId) : undefined}
            />
            {#if isExpanded(entry.group)}
              <div class="bg-yt-surface/20 divide-y divide-yt-border/30">
                {#each (groupItems.get(entry.group.groupId) ?? []) as item (item.id)}
                  {@render queueItemCard(item, true)}
                {/each}
              </div>
            {/if}
          {:else}
            {@render queueItemCard(entry.item, false)}
          {/if}
        {/each}
      </div>

      <!-- Pagination -->
      {#if totalPages > 1}
        <div class="flex items-center justify-center gap-2 py-4 border-t border-yt-border">
          <button
            class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            onclick={() => goToPage(currentPage - 1)}
            disabled={currentPage === 0}
          >
            <span class="material-symbols-outlined text-[16px]">chevron_left</span>
          </button>
          <span class="text-xs text-yt-text-secondary px-2">
            {currentPage + 1} / {totalPages}
          </span>
          <button
            class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text text-xs font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
            onclick={() => goToPage(currentPage + 1)}
            disabled={currentPage >= totalPages - 1}
          >
            <span class="material-symbols-outlined text-[16px]">chevron_right</span>
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>
