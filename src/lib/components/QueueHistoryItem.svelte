<script lang="ts">
  import type { HistoryItem } from "$lib/bindings"
  import { t, getDateLocale } from "$lib/i18n/index.svelte"
  import { formatSize } from "$lib/utils/format"

  let {
    item,
    deleteBusy,
    onOpen,
    onReveal,
    onDelete,
  }: {
    item: HistoryItem
    deleteBusy: boolean
    onOpen: () => void
    onReveal: () => void
    onDelete: () => void
  } = $props()

  const thumbnail = $derived(item.videoId.trim() ? `https://i.ytimg.com/vi/${item.videoId}/mqdefault.jpg` : null)
  const downloadedAt = $derived(new Date(item.downloadedAt * 1000).toLocaleString(getDateLocale(), {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }))

  function hideThumb(e: Event) {
    (e.currentTarget as HTMLImageElement).remove()
  }
</script>

<div class="group flex items-center gap-3 px-3 py-2.5 rounded-lg hover:bg-yt-highlight/40 transition-colors">
  <div class="relative w-16 h-10 rounded overflow-hidden bg-yt-overlay-subtle shrink-0">
    <span class="absolute inset-0 flex items-center justify-center material-symbols-outlined text-yt-success/60 text-[18px]">check_circle</span>
    {#if thumbnail}
      <img src={thumbnail} alt="" loading="lazy" class="absolute inset-0 w-full h-full object-cover" onerror={hideThumb} />
    {/if}
  </div>
  <div class="flex-1 min-w-0">
    <h4 class="font-medium text-yt-text text-sm truncate mb-0.5">{item.title}</h4>
    <div class="flex items-center gap-2 text-xs text-yt-text-secondary flex-wrap">
      <span class="px-1.5 py-0.5 rounded bg-yt-overlay">{item.qualityLabel || "N/A"}</span>
      <span class="px-1.5 py-0.5 rounded bg-yt-overlay">{item.format}</span>
      <span>{formatSize(item.fileSize, "-")}</span>
      <span>{downloadedAt}</span>
    </div>
  </div>
  <div class="flex items-center gap-1 shrink-0">
    {#if item.filePath}
      <button
        class="opacity-0 group-hover:opacity-100 text-yt-text-muted hover:text-yt-primary transition-all p-1.5 rounded-md hover:bg-yt-primary/10"
        onclick={onOpen}
        aria-label={t("history.openFile")}
        title={t("history.openFile")}
      >
        <span class="material-symbols-outlined text-[18px]">play_arrow</span>
      </button>
      <button
        class="opacity-0 group-hover:opacity-100 text-yt-text-muted hover:text-yt-primary transition-all p-1.5 rounded-md hover:bg-yt-primary/10"
        onclick={onReveal}
        aria-label={t("history.revealInFolder")}
        title={t("history.revealInFolder")}
      >
        <span class="material-symbols-outlined text-[18px]">folder_open</span>
      </button>
    {/if}
    <button
      class="opacity-0 group-hover:opacity-100 text-yt-text-muted hover:text-yt-error transition-all p-1.5 rounded-md hover:bg-yt-error/10"
      onclick={onDelete}
      disabled={deleteBusy}
      aria-label={t("history.deleteItem")}
      title={t("history.deleteItem")}
    >
      <span class="material-symbols-outlined text-[18px]">delete</span>
    </button>
  </div>
</div>
