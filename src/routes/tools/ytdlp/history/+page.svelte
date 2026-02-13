<script lang="ts">
  import { commands } from "$lib/bindings"
  import { onMount } from "svelte"

  let items = $state<any[]>([])
  let totalCount = $state(0)
  let currentPage = $state(0)
  let pageSize = $state(20)
  let search = $state("")
  let loading = $state(true)
  let searchTimeout: ReturnType<typeof setTimeout>

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

  async function handleDelete(id: number) {
    if (!confirm("이 항목을 삭제하시겠습니까?")) return
    const result = await commands.deleteHistoryItem(id)
    if (result.status === "ok") await loadHistory()
  }

  function prevPage() { if (currentPage > 0) { currentPage--; loadHistory() } }
  function nextPage() { if (currentPage < totalPages - 1) { currentPage++; loadHistory() } }

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleString("ko-KR", { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })
  }
  function formatSize(bytes: number | null): string {
    if (!bytes) return "-"
    const mb = bytes / (1024 ** 2)
    if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
    if (mb >= 1) return `${mb.toFixed(1)} MB`
    return `${(bytes / 1024).toFixed(1)} KB`
  }
</script>

<div class="flex-1 flex flex-col h-full overflow-y-auto hide-scrollbar">
  <header class="px-6 py-4 shrink-0">
    <h2 class="text-xl font-display font-bold text-gray-900">Library</h2>
    <p class="text-gray-400 mt-1">Your download history</p>
  </header>

  <!-- Search -->
  <div class="px-6 mb-4">
    <div class="relative">
      <div class="absolute inset-y-0 left-4 flex items-center pointer-events-none text-gray-400">
        <span class="material-symbols-outlined text-[20px]">search</span>
      </div>
      <input
        type="text"
        class="w-full h-10 bg-yt-highlight text-gray-900 rounded-xl pl-12 pr-4 border border-gray-200 focus:ring-2 focus:ring-yt-primary focus:outline-none placeholder-gray-400 text-sm"
        placeholder="제목으로 검색..."
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
        <span class="material-symbols-outlined text-gray-300 text-6xl">library_books</span>
        <p class="text-gray-500 mt-4 text-lg">다운로드 이력이 없습니다</p>
      </div>
    {:else}
      {#each items as item}
        <div class="bg-yt-highlight rounded-xl p-4 flex gap-4 items-center group hover:bg-gray-100 transition-colors border border-gray-200">
          <div class="w-20 h-14 bg-gray-100 rounded-lg overflow-hidden shrink-0 relative">
            <div class="w-full h-full bg-gradient-to-br from-gray-50 to-gray-200 flex items-center justify-center">
              <span class="material-symbols-outlined text-green-600/60">check_circle</span>
            </div>
          </div>

          <div class="flex-1 min-w-0">
            <h4 class="font-medium text-gray-900 text-sm truncate mb-1">{item.title}</h4>
            <div class="flex items-center gap-3 text-xs text-gray-400">
              <span class="px-2 py-0.5 rounded bg-gray-200 text-gray-600">{item.qualityLabel || "N/A"}</span>
              <span class="px-2 py-0.5 rounded bg-gray-200 text-gray-600">{item.format}</span>
              <span>{formatSize(item.fileSize)}</span>
              <span class="text-gray-400">{formatDate(item.downloadedAt)}</span>
            </div>
          </div>

          <button
            class="opacity-0 group-hover:opacity-100 text-gray-400 hover:text-red-600 transition-all p-2 rounded-lg hover:bg-red-500/10"
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
            class="px-4 py-2 rounded-xl bg-yt-highlight text-gray-600 hover:bg-gray-100 transition-colors disabled:opacity-50 border border-gray-200"
            onclick={prevPage}
            disabled={currentPage === 0}
          >
            <span class="material-symbols-outlined text-[18px]">chevron_left</span>
          </button>
          <span class="text-sm text-gray-500">
            {currentPage + 1} / {totalPages}
          </span>
          <button
            class="px-4 py-2 rounded-xl bg-yt-highlight text-gray-600 hover:bg-gray-100 transition-colors disabled:opacity-50 border border-gray-200"
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
