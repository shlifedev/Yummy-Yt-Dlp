<script lang="ts">
  // Shared batch-group header for the unified queue page (both the "in progress"
  // and "records" sections). The parent controls the subtitle text, whether a
  // progress bar shows, and the trailing action (cancel / delete).
  let {
    title,
    subtitle,
    progress = null,
    expanded = false,
    onToggle,
    onAction,
    actionIcon = "close",
    actionTitle = "",
  }: {
    title: string
    subtitle: string
    progress?: number | null
    expanded?: boolean
    onToggle: () => void
    onAction?: () => void
    actionIcon?: string
    actionTitle?: string
  } = $props()
</script>

<div class="group flex items-center gap-3 px-3 py-2.5 rounded-lg bg-yt-surface border border-yt-border/60">
  <!-- Toggle area. Sibling of the action button to avoid nested <button>. -->
  <button class="flex items-center gap-3 flex-1 min-w-0 text-left" onclick={onToggle}>
    <span
      class="material-symbols-outlined text-yt-text-muted text-[20px] transition-transform shrink-0 {expanded ? '' : '-rotate-90'}"
    >expand_more</span>
    <div class="shrink-0 w-8 h-8 rounded-full bg-yt-primary/10 flex items-center justify-center">
      <span class="material-symbols-outlined text-yt-primary text-[18px]">playlist_play</span>
    </div>
    <div class="flex-1 min-w-0">
      <div class="flex items-center justify-between gap-3 {progress !== null ? 'mb-1' : ''}">
        <h4 class="font-semibold text-yt-text text-sm truncate">{title}</h4>
        <span class="text-xs text-yt-text-secondary whitespace-nowrap shrink-0">{subtitle}</span>
      </div>
      {#if progress !== null}
        <div class="w-full bg-yt-overlay rounded-full h-1.5 overflow-hidden relative">
          <div class="bg-yt-primary h-full transition-all duration-300 relative overflow-hidden" style="width: {progress}%">
            <div class="absolute inset-0 animate-shimmer"></div>
          </div>
        </div>
      {/if}
    </div>
  </button>

  {#if onAction}
    <button
      class="shrink-0 p-1.5 rounded-md hover:bg-yt-error/10 text-yt-text-muted hover:text-yt-error transition-colors opacity-0 group-hover:opacity-100"
      onclick={onAction}
      title={actionTitle}
    >
      <span class="material-symbols-outlined text-[18px]">{actionIcon}</span>
    </button>
  {/if}
</div>
