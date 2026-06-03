<script lang="ts">
  import { goto } from "$app/navigation"
  import { onMount } from "svelte"

  // Queue and history were merged into the unified /queue view. Keep this route as a redirect so
  // old bookmarks or direct links still land somewhere useful.
  onMount(() => { goto("/tools/ytdlp/queue", { replaceState: true }) })
</script>

<div class="flex-1 flex items-center justify-center">
  <span class="material-symbols-outlined text-yt-primary text-3xl animate-spin">progress_activity</span>
</div>
