<script lang="ts">
  import { commands } from "$lib/bindings"
  import { onMount, onDestroy } from "svelte"
  import { t, getDateLocale } from "$lib/i18n/index.svelte"

  let items = $state<any[]>([])
  let totalCount = $state(0)
  let currentPage = $state(0)
  let pageSize = $state(20)
  let search = $state("")
  let loading = $state(true)
  let searchTimeout: ReturnType<typeof setTimeout>

  // 5-2: Clean up searchTimeout on unmount
  onDestroy(() => { clearTimeout(searchTimeout) })

  let totalPages = $derived(Math.ceil(totalCount / pageSize))

  onMount(async () => { await loadHistory() })

  async function loadHistory() {
    loading = true
    try {
      const result = await commands.getDownloadHistory(currentPage, pageSize, search || null)
      if (result.status === "ok") {
        items = result.data.items
        totalCount = result.data.totalCount
      }
    } catch (e) { console.error(e) }
    finally { loading = false }
  }

  function handleSearch(value: string) {
    clearTimeout(searchTimeout)
    search = value
    searchTimeout = setTimeout(() => { currentPage = 0; loadHistory() }, 300)
  }

  // 4-2: Add try/catch to prevent unhandled errors
  async function handleDelete(id: number) {
    if (!confirm(t("history.deleteConfirm"))) return
    try {
      const result = await commands.deleteHistoryItem(id)
      if (result.status === "ok") await loadHistory()
    } catch (e) {
      console.error("Failed to delete history item:", e)
    }
  }

  function prevPage() { if (currentPage > 0) { currentPage--; loadHistory() } }
  function nextPage() { if (currentPage < totalPages - 1) { currentPage++; loadHistory() } }

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleString(getDateLocale(), { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })
  }
  // 5-3: Fix formatSize(0) returning "-"
  function formatSize(bytes: number | null): string {
    if (bytes === null || bytes === undefined) return "-"
    if (bytes === 0) return "0 MB"
    const mb = bytes / (1024 ** 2)
    if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
    if (mb >= 1) return `${mb.toFixed(1)} MB`
    return `${(bytes / 1024).toFixed(1)} KB`
  }
</script>

<div class="flex-1 flex flex-col h-full overflow-y-auto hide-scrollbar">
  <header class="px-6 py-4 shrink-0">
    <h2 class="text-xl font-display font-bold text-gray-100">{t("history.title")}</h2>
    <p class="text-gray-400 mt-1">{t("history.subtitle")}</p>
  </header>

  <!-- Search -->
  <div class="px-6 mb-4">
    <div class="relative">
      <div class="absolute inset-y-0 left-4 flex items-center pointer-events-none text-gray-400">
        <span class="material-symbols-outlined text-[20px]">search</span>
      </div>
      <input
        type="text"
        class="w-full h-10 bg-yt-highlight text-gray-100 rounded-xl pl-12 pr-4 border border-white/[0.06] focus:ring-2 focus:ring-yt-primary focus:outline-none placeholder-gray-600 text-sm"
        placeholder={t("history.searchPlaceholder")}
        value={search}
        oninput={(e) => handleSearch((e.target as HTMLInputElement).value)}
      />
    </div>
  </div>

  <div class="px-6 pb-6 space-y-3 flex-1">
    {#if loading}
      <div class="flex justify-center py-16">
        <span class="material-symbols-outlined text-yt-primary text-4xl animate-spin">progress_activity</span>
      </div>
    {:else if items.length === 0}
      <div class="flex flex-col items-center justify-center py-20">
        <span class="material-symbols-outlined text-gray-600 text-6xl">library_books</span>
        <p class="text-gray-400 mt-4 text-lg">{t("history.empty")}</p>
      </div>
    {:else}
      {#each items as item (item.id)}
        <div class="bg-yt-highlight rounded-xl p-4 flex gap-4 items-center group hover:bg-white/[0.06] transition-colors border border-white/[0.06]">
          <div class="w-20 h-14 bg-white/[0.04] rounded-lg overflow-hidden shrink-0 relative">
            <div class="w-full h-full bg-gradient-to-br from-white/[0.03] to-white/[0.08] flex items-center justify-center">
              <span class="material-symbols-outlined text-green-600/60">check_circle</span>
            </div>
          </div>

          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-gray-100 text-sm truncate mb-1">{item.title}</h4>
            <div class="flex items-center gap-3 text-xs text-gray-400">
              <span class="px-2 py-0.5 rounded bg-white/[0.06] text-gray-400">{item.qualityLabel || "N/A"}</span>
              <span class="px-2 py-0.5 rounded bg-white/[0.06] text-gray-400">{item.format}</span>
              <span>{formatSize(item.fileSize)}</span>
              <span class="text-gray-400">{formatDate(item.downloadedAt)}</span>
            </div>
          </div>

          <button
            class="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-red-400 transition-all p-2 rounded-lg hover:bg-red-500/10"
            onclick={() => handleDelete(item.id)}
          >
            <span class="material-symbols-outlined text-[20px]">delete</span>
          </button>
        </div>
      {/each}

      <!-- Pagination -->
      {#if totalPages > 1}
        <div class="flex justify-center items-center gap-4 pt-4">
          <button
            class="px-4 py-2 rounded-xl bg-yt-highlight text-gray-400 hover:bg-white/[0.06] transition-colors disabled:opacity-50 border border-white/[0.06]"
            onclick={prevPage}
            disabled={currentPage === 0}
          >
            <span class="material-symbols-outlined text-[18px]">chevron_left</span>
          </button>
          <span class="text-sm text-gray-400">
            {currentPage + 1} / {totalPages}
          </span>
          <button
            class="px-4 py-2 rounded-xl bg-yt-highlight text-gray-400 hover:bg-white/[0.06] transition-colors disabled:opacity-50 border border-white/[0.06]"
            onclick={nextPage}
            disabled={currentPage >= totalPages - 1}
          >
            <span class="material-symbols-outlined text-[18px]">chevron_right</span>
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>
