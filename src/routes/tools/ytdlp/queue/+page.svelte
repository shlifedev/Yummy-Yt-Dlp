<script lang="ts">
  import { commands } from "$lib/bindings"
  import { onMount } from "svelte"

  let queue = $state<any[]>([])
  let firstLoad = $state(true)

  onMount(async () => {
    await loadQueue()
    firstLoad = false
  })

  async function loadQueue() {
    try {
      const result = await commands.getDownloadQueue()
      if (result.status === "ok") {
        queue = result.data
      }
    } catch (e) {
      console.error("Failed to load queue:", e)
    }
  }

  async function handleClearCompleted() {
    const result = await commands.clearCompleted()
    if (result.status === "ok") await loadQueue()
  }

  async function handleCancel(id: number) {
    await commands.cancelDownload(id)
    await loadQueue()
  }

  // Auto-refresh every 2 seconds
  let interval: ReturnType<typeof setInterval>
  onMount(() => {
    interval = setInterval(loadQueue, 2000)
    return () => clearInterval(interval)
  })

  let activeCount = $derived(queue.filter(q => q.status === "downloading").length)
  let completedCount = $derived(queue.filter(q => q.status === "completed").length)

  function formatSize(bytes: number | null): string {
    if (!bytes) return ""
    const mb = bytes / (1024 ** 2)
    if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
    return `${Math.round(mb)} MB`
  }
</script>

<div class="flex-1 flex flex-col h-full overflow-y-auto hide-scrollbar">
  <header class="px-6 py-4 shrink-0">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-xl font-display font-bold text-gray-900">Downloads</h2>
        <p class="text-gray-400 mt-1">Manage your download queue</p>
      </div>
      <button
        class="px-4 py-2 rounded-xl bg-red-500/10 text-red-600 hover:bg-red-500/20 text-sm font-medium transition-colors disabled:opacity-50"
        onclick={handleClearCompleted}
        disabled={completedCount === 0}
      >
        <span class="material-symbols-outlined text-[18px] align-middle mr-1">delete_sweep</span>
        Clear Completed
      </button>
    </div>

    <!-- Stats -->
    <div class="flex gap-4 mt-4">
      <div class="bg-yt-highlight rounded-xl px-4 py-2 flex items-center gap-2 border border-gray-200">
        <span class="material-symbols-outlined text-yt-primary text-[18px]">downloading</span>
        <span class="text-sm"><span class="font-bold text-gray-900">{activeCount}</span> <span class="text-gray-400">Active</span></span>
      </div>
      <div class="bg-yt-highlight rounded-xl px-4 py-2 flex items-center gap-2 border border-gray-200">
        <span class="material-symbols-outlined text-green-600 text-[18px]">check_circle</span>
        <span class="text-sm"><span class="font-bold text-gray-900">{completedCount}</span> <span class="text-gray-400">Completed</span></span>
      </div>
      <div class="bg-yt-highlight rounded-xl px-4 py-2 flex items-center gap-2 border border-gray-200">
        <span class="material-symbols-outlined text-gray-400 text-[18px]">list</span>
        <span class="text-sm"><span class="font-bold text-gray-900">{queue.length}</span> <span class="text-gray-400">Total</span></span>
      </div>
    </div>
  </header>

  <div class="px-6 pb-6 space-y-3 flex-1">
    {#if firstLoad}
      <div class="flex justify-center py-16">
        <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
      </div>
    {:else if queue.length === 0}
      <div class="flex flex-col items-center justify-center py-20 text-center">
        <span class="material-symbols-outlined text-gray-300 text-6xl">inbox</span>
        <p class="text-gray-500 mt-4 text-lg">대기열이 비어 있습니다</p>
        <p class="text-gray-400 text-sm mt-1">홈에서 다운로드를 시작하세요</p>
      </div>
    {:else}
      {#each queue as item (item.id)}
        <div class="bg-yt-highlight rounded-xl p-4 flex gap-4 items-center hover:bg-gray-100 transition-colors border border-gray-200
          {item.status === 'downloading' ? '!border-yt-primary/30 relative overflow-hidden' : ''}">
          {#if item.status === "downloading"}
            <div class="absolute bottom-0 left-0 h-1 bg-yt-primary" style="width: {item.progress || 0}%"></div>
          {/if}

          <div class="w-20 h-14 bg-gray-100 rounded-lg overflow-hidden shrink-0 relative">
            <div class="w-full h-full bg-gradient-to-br from-gray-50 to-gray-200 flex items-center justify-center">
              {#if item.status === "downloading"}
                <span class="material-symbols-outlined text-yt-primary animate-spin">progress_activity</span>
              {:else if item.status === "completed"}
                <span class="material-symbols-outlined text-green-600">check_circle</span>
              {:else if item.status === "failed"}
                <span class="material-symbols-outlined text-red-600">error</span>
              {:else}
                <span class="material-symbols-outlined text-gray-400">download</span>
              {/if}
            </div>
          </div>

          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-gray-900 text-sm truncate mb-1">{item.title}</h4>
            <div class="flex items-center gap-3 text-xs text-gray-400">
              <span class="px-2 py-0.5 rounded bg-gray-200 text-gray-600">{item.qualityLabel || "N/A"}</span>
              {#if item.status === "downloading" && item.speed}
                <span class="text-yt-primary font-mono">{item.speed}</span>
                <span>ETA: {item.eta || "..."}</span>
              {/if}
            </div>
            {#if item.status === "downloading"}
              <div class="w-full bg-gray-200 rounded-full h-1.5 mt-2">
                <div class="bg-yt-primary h-1.5 rounded-full transition-all" style="width: {item.progress || 0}%"></div>
              </div>
            {/if}
          </div>

          <div class="text-right shrink-0 flex items-center gap-3">
            {#if item.status === "completed"}
              <span class="flex items-center gap-1.5 text-green-600 text-xs font-medium">
                <span class="material-symbols-outlined text-[16px]">check_circle</span>
                Completed
              </span>
            {:else if item.status === "downloading"}
              <span class="text-gray-900 text-sm font-bold font-mono">{(item.progress || 0).toFixed(0)}%</span>
              <button class="text-gray-400 hover:text-red-600 transition-colors" onclick={() => handleCancel(item.id)}>
                <span class="material-symbols-outlined text-[20px]">close</span>
              </button>
            {:else if item.status === "failed"}
              <span class="flex items-center gap-1.5 text-red-600 text-xs font-medium">
                <span class="material-symbols-outlined text-[16px]">error</span>
                Failed
              </span>
            {:else}
              <span class="text-gray-400 text-xs">Pending</span>
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>
</div>
