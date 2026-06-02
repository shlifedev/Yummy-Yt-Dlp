<script lang="ts">
  import { commands } from "$lib/bindings"
  import { onMount, onDestroy } from "svelte"
  import { t, getDateLocale } from "$lib/i18n/index.svelte"
  import { formatSize } from "$lib/utils/format"

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
        // Stepped past the end after deleting the last item on a page? Clamp back so the user
        // isn't stranded on an empty page while earlier pages still have history.
        if (items.length === 0 && currentPage > 0 && totalCount > 0) {
          currentPage = Math.max(0, Math.ceil(totalCount / pageSize) - 1)
          return loadHistory()
        }
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
</script>

<div class="flex-1 flex flex-col h-full overflow-y-auto hide-scrollbar">
  <header class="px-6 py-4 shrink-0">
    <h2 class="text-xl font-display font-bold text-yt-text">{t("history.title")}</h2>
    <p class="text-yt-text-secondary mt-1">{t("history.subtitle")}</p>
  </header>

  <!-- Search -->
  <div class="px-6 mb-4">
    <div class="relative">
      <div class="absolute inset-y-0 left-4 flex items-center pointer-events-none text-yt-text-secondary">
        <span class="material-symbols-outlined text-[20px]">search</span>
      </div>
      <label for="history-search" class="sr-only">Search history</label>
      <input
        id="history-search"
        type="text"
        class="w-full h-10 bg-yt-highlight text-yt-text rounded-xl pl-12 pr-4 border border-yt-border focus:ring-2 focus:ring-yt-primary focus:outline-none text-sm"
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
        <span class="material-symbols-outlined text-yt-text-muted text-6xl">library_books</span>
        <p class="text-yt-text-secondary mt-4 text-lg">{t("history.empty")}</p>
      </div>
    {:else}
      {#each items as item (item.id)}
        <div class="bg-yt-highlight rounded-xl p-4 flex gap-4 items-center group hover:bg-yt-overlay transition-colors border border-yt-border">
          <div class="w-20 h-14 bg-yt-overlay-subtle rounded-lg overflow-hidden shrink-0 relative">
            <div class="w-full h-full bg-gradient-to-br from-yt-overlay-subtle to-yt-overlay-strong flex items-center justify-center">
              <span class="material-symbols-outlined text-yt-success/60">check_circle</span>
            </div>
          </div>

          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-yt-text text-sm truncate mb-1">{item.title}</h4>
            <div class="flex items-center gap-3 text-xs text-yt-text-secondary">
              <span class="px-2 py-0.5 rounded bg-yt-overlay text-yt-text-secondary">{item.qualityLabel || "N/A"}</span>
              <span class="px-2 py-0.5 rounded bg-yt-overlay text-yt-text-secondary">{item.format}</span>
              <span>{formatSize(item.fileSize, "-")}</span>
              <span class="text-yt-text-secondary">{formatDate(item.downloadedAt)}</span>
            </div>
          </div>

          <button
            class="opacity-0 group-hover:opacity-100 text-yt-text-secondary hover:text-yt-error transition-all p-2 rounded-lg hover:bg-yt-error/10"
            onclick={() => handleDelete(item.id)}
            aria-label="Delete"
          >
            <span class="material-symbols-outlined text-[20px]">delete</span>
          </button>
        </div>
      {/each}

      <!-- Pagination -->
      {#if totalPages > 1}
        <div class="flex justify-center items-center gap-4 pt-4">
          <button
            class="px-4 py-2 rounded-xl bg-yt-highlight text-yt-text-secondary hover:bg-yt-overlay transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-yt-border"
            onclick={prevPage}
            disabled={currentPage === 0}
            aria-label="Previous page"
          >
            <span class="material-symbols-outlined text-[18px]">chevron_left</span>
          </button>
          <span class="text-sm text-yt-text-secondary">
            {currentPage + 1} / {totalPages}
          </span>
          <button
            class="px-4 py-2 rounded-xl bg-yt-highlight text-yt-text-secondary hover:bg-yt-overlay transition-colors disabled:opacity-50 disabled:cursor-not-allowed border border-yt-border"
            onclick={nextPage}
            disabled={currentPage >= totalPages - 1}
            aria-label="Next page"
          >
            <span class="material-symbols-outlined text-[18px]">chevron_right</span>
          </button>
        </div>
      {/if}
    {/if}
  </div>
</div>
