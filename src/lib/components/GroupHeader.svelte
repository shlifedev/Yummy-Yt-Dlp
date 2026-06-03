<script lang="ts">
  import { t } from "$lib/i18n/index.svelte"

  let {
    title,
    completedCount,
    totalCount,
    progress = 0,
    expanded = false,
    variant = "queue",
    onToggle,
    onAction,
  }: {
    title: string
    completedCount: number
    totalCount: number
    progress?: number
    expanded?: boolean
    variant?: "queue" | "history"
    onToggle: () => void
    onAction?: () => void
  } = $props()
</script>

<div class="group flex items-center gap-3 px-6 py-3 hover:bg-yt-highlight/30 transition-colors">
  <!-- Toggle area (expand/collapse). Sibling of the action button to avoid nested <button>. -->
  <button class="flex items-center gap-3 flex-1 min-w-0 text-left" onclick={onToggle}>
    <span
      class="material-symbols-outlined text-yt-text-muted text-[20px] transition-transform shrink-0 {expanded ? '' : '-rotate-90'}"
    >expand_more</span>
    <div class="shrink-0 w-8 h-8 rounded-full bg-yt-primary/10 flex items-center justify-center">
      <span class="material-symbols-outlined text-yt-primary text-[18px]">
        {variant === "history" ? "library_music" : "playlist_play"}
      </span>
    </div>
    <div class="flex-1 min-w-0">
      <div class="flex items-center justify-between gap-3 {variant === 'queue' ? 'mb-1' : ''}">
        <h4 class="font-semibold text-yt-text text-sm truncate">{title}</h4>
        <span class="text-xs text-yt-text-secondary whitespace-nowrap shrink-0">
          {t("queue.groupProgress", { completed: completedCount, total: totalCount })}
        </span>
      </div>
      {#if variant === "queue"}
        <div class="w-full bg-yt-surface rounded-full h-1.5 border border-yt-border/50 overflow-hidden relative">
          <div class="bg-yt-primary h-full transition-all duration-300 relative overflow-hidden" style="width: {progress}%">
            <div class="absolute inset-0 animate-shimmer"></div>
          </div>
        </div>
      {/if}
    </div>
  </button>

  <!-- Action: cancel (queue) / delete (history) -->
  {#if onAction}
    <button
      class="shrink-0 p-1.5 rounded-md hover:bg-yt-error/10 text-yt-text-muted hover:text-yt-error transition-colors opacity-0 group-hover:opacity-100"
      onclick={onAction}
      title={variant === "history" ? t("history.deleteGroup") : t("queue.cancelGroup")}
    >
      <span class="material-symbols-outlined text-[18px]">{variant === "history" ? "delete" : "close"}</span>
    </button>
  {/if}
</div>
