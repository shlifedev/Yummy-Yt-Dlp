<script lang="ts">
  import type { DownloadTaskInfo } from "$lib/bindings"
  import { t } from "$lib/i18n/index.svelte"
  import { fade } from "svelte/transition"

  let {
    item,
    errorExpanded,
    cancelBusy,
    retryBusy,
    onCancel,
    onRetry,
    onToggleError,
  }: {
    item: DownloadTaskInfo
    errorExpanded: boolean
    cancelBusy: boolean
    retryBusy: boolean
    onCancel: () => void
    onRetry: () => void
    onToggleError: () => void
  } = $props()

  const thumbnail = $derived(item.videoId.trim() ? `https://i.ytimg.com/vi/${item.videoId}/mqdefault.jpg` : null)

  function hideThumb(e: Event) {
    (e.currentTarget as HTMLImageElement).remove()
  }
</script>

<div class="flex items-center gap-3 px-3 py-2.5 rounded-lg bg-yt-surface border border-yt-border/60" in:fade>
  <div class="relative w-16 h-10 rounded overflow-hidden bg-yt-overlay-subtle shrink-0">
    <span class="absolute inset-0 flex items-center justify-center material-symbols-outlined text-yt-text-muted text-[18px]">movie</span>
    {#if thumbnail}
      <img src={thumbnail} alt="" loading="lazy" class="absolute inset-0 w-full h-full object-cover" onerror={hideThumb} />
    {/if}
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
        <button class="text-yt-error hover:underline flex items-center gap-1" onclick={onToggleError}>
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
    {#if item.status === "failed" && (item.errorDetail || item.errorMessage) && errorExpanded}
      <div class="mt-2 text-xs text-yt-error bg-yt-error/5 p-2 rounded border border-yt-error/10 font-mono whitespace-pre-wrap">{item.errorDetail ?? (item.errorMessage ? t(item.errorMessage) : "")}</div>
    {/if}
  </div>

  <div class="shrink-0">
    {#if item.status === "downloading" || item.status === "pending"}
      <button class="p-1.5 rounded-md hover:bg-yt-error/10 text-yt-text-muted hover:text-yt-error transition-colors disabled:opacity-40 disabled:cursor-not-allowed" onclick={onCancel} disabled={cancelBusy} title={t("download.cancel")}>
        <span class="material-symbols-outlined text-[18px] {cancelBusy ? 'animate-spin' : ''}">{cancelBusy ? "progress_activity" : "close"}</span>
      </button>
    {:else if item.status === "failed" || item.status === "cancelled"}
      <button class="p-1.5 rounded-md hover:bg-yt-primary/10 text-yt-text-muted hover:text-yt-primary transition-colors disabled:opacity-40 disabled:cursor-not-allowed" onclick={onRetry} disabled={retryBusy} title={t("queue.retry")}>
        <span class="material-symbols-outlined text-[18px] {retryBusy ? 'animate-spin' : ''}">{retryBusy ? "progress_activity" : "refresh"}</span>
      </button>
    {/if}
  </div>
</div>
