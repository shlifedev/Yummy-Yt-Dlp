<script lang="ts">
  import { commands, type PlaylistResult } from "$lib/bindings"
  import { listen } from "@tauri-apps/api/event"
  import { onMount, onDestroy } from "svelte"

  // URL & analyze state
  let url = $state("")
  let analyzing = $state(false)
  let videoInfo = $state<any>(null)
  let error = $state<string | null>(null)
  let playlistResult = $state<PlaylistResult | null>(null)
  let playlistPage = $state(0)
  let loadingMore = $state(false)

  // Download options
  let format = $state<"mp4" | "mkv" | "mp3">("mp4")
  let quality = $state("best")
  let embedSubs = $state(true)

  // Download state
  let downloading = $state(false)
  let downloadStatus = $state<string>("idle")
  let progress = $state(0)
  let speed = $state("")
  let eta = $state("")
  let taskId = $state<number | null>(null)

  // Settings
  let downloadPath = $state("~/Downloads")

  let unlisten: (() => void) | null = null

  onMount(async () => {
    await loadSettings()

    // Listen for global download events
    unlisten = await listen("download-event", (event: any) => {
      const data = event.payload
      if (data.taskId === taskId) {
        switch (data.eventType) {
          case "started":
            downloadStatus = "downloading"
            downloading = true
            break
          case "progress":
            progress = data.percent || 0
            speed = data.speed || ""
            eta = data.eta || ""
            break
          case "completed":
            downloadStatus = "completed"
            downloading = false
            progress = 100
            break
          case "error":
            downloadStatus = "failed"
            downloading = false
            error = data.message || "다운로드 실패"
            break
        }
      }
    })
  })

  onDestroy(() => {
    if (unlisten) unlisten()
  })

  async function loadSettings() {
    try {
      const result = await commands.getSettings()
      if (result.status === "ok") {
        downloadPath = result.data.downloadPath
      }
    } catch {}
  }

  function extractError(err: any): string {
    if (typeof err === "string") return err
    const values = Object.values(err)
    return (values[0] as string) || "알 수 없는 오류"
  }

  async function handlePaste() {
    try {
      const text = await navigator.clipboard.readText()
      if (text) url = text
    } catch {}
  }

  async function handleAnalyze() {
    if (!url.trim()) return
    analyzing = true
    error = null
    videoInfo = null
    playlistResult = null
    playlistPage = 0

    try {
      const valResult = await commands.validateUrl(url)
      if (valResult.status === "error") {
        error = extractError(valResult.error)
        return
      }
      if (!valResult.data.valid) {
        error = "유효하지 않은 YouTube URL입니다"
        return
      }

      const normalized = valResult.data.normalizedUrl || url

      if (valResult.data.urlType === "video") {
        const infoResult = await commands.fetchVideoInfo(normalized)
        if (infoResult.status === "error") {
          error = extractError(infoResult.error)
          return
        }
        videoInfo = infoResult.data
      } else if (valResult.data.urlType === "channel" || valResult.data.urlType === "playlist") {
        const plResult = await commands.fetchPlaylistInfo(normalized, 0, 50)
        if (plResult.status === "error") {
          error = extractError(plResult.error)
          return
        }
        playlistResult = plResult.data
      }
    } catch (e: any) {
      error = e.message || String(e)
    } finally {
      analyzing = false
    }
  }

  async function handleLoadMore() {
    if (!playlistResult || loadingMore) return
    loadingMore = true
    try {
      const nextPage = playlistPage + 1
      const result = await commands.fetchPlaylistInfo(playlistResult.url, nextPage, 50)
      if (result.status === "ok" && result.data.entries.length > 0) {
        playlistResult = {
          ...playlistResult,
          entries: [...playlistResult.entries, ...result.data.entries],
        }
        playlistPage = nextPage
      }
    } catch (e: any) {
      error = e.message || String(e)
    } finally {
      loadingMore = false
    }
  }

  async function handleSelectVideo(entry: { url: string; videoId: string; title: string | null }) {
    analyzing = true
    error = null
    try {
      const infoResult = await commands.fetchVideoInfo(entry.url)
      if (infoResult.status === "error") {
        error = extractError(infoResult.error)
        return
      }
      videoInfo = infoResult.data
    } catch (e: any) {
      error = e.message || String(e)
    } finally {
      analyzing = false
    }
  }

  function buildFormatString(): string {
    if (format === "mp3") return "bestaudio/best"
    let h = ""
    if (quality === "1080p") h = "[height<=1080]"
    else if (quality === "720p") h = "[height<=720]"
    else if (quality === "480p") h = "[height<=480]"
    if (format === "mp4") return `bestvideo${h}[ext=mp4]+bestaudio[ext=m4a]/best${h}[ext=mp4]/best${h}`
    return `bestvideo${h}+bestaudio/best${h}`
  }

  async function handleStartDownload() {
    if (!videoInfo && !url.trim()) return
    downloading = true
    downloadStatus = "downloading"
    progress = 0
    speed = ""
    eta = ""
    error = null

    try {
      const request = {
        videoUrl: videoInfo?.url || url,
        videoId: videoInfo?.videoId || "",
        title: videoInfo?.title || url,
        formatId: buildFormatString(),
        qualityLabel: quality === "best" ? "Best" : quality,
        outputDir: null,
        cookieBrowser: null,
      }

      const result = await commands.addToQueue(request)
      if (result.status === "error") {
        downloadStatus = "failed"
        downloading = false
        error = extractError(result.error)
      } else {
        taskId = result.data
      }
    } catch (e: any) {
      downloadStatus = "failed"
      downloading = false
      error = e.message || String(e)
    }
  }

  async function handleCancelDownload() {
    if (taskId) {
      try {
        await commands.cancelDownload(taskId)
        downloadStatus = "cancelled"
        downloading = false
      } catch {}
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !analyzing && !downloading) handleAnalyze()
  }

  function formatDuration(seconds: number): string {
    const m = Math.floor(seconds / 60)
    const s = seconds % 60
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
  }

  function formatSize(bytes: number | null): string {
    if (!bytes) return ""
    const mb = bytes / (1024 ** 2)
    if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
    return `${Math.round(mb)} MB`
  }
</script>

<div class="h-full overflow-y-auto hide-scrollbar">
    <!-- Header -->
    <header class="px-6 py-4 flex justify-between items-center shrink-0">
      <div>
        <h2 class="text-xl font-display font-bold text-gray-900">Dashboard</h2>
        <p class="text-gray-400 mt-1">Ready to capture some content?</p>
      </div>
      <div class="flex items-center gap-4">
        <div class="h-6 w-px bg-gray-200"></div>
        <div class="flex items-center gap-2 text-sm text-green-600 bg-green-500/10 px-3 py-1.5 rounded-full">
          <span class="w-2 h-2 bg-green-500 rounded-full animate-pulse"></span>
          Engine Ready
        </div>
      </div>
    </header>

    <div class="px-6 pb-6 space-y-4">
      <!-- Error -->
      {#if error}
        <div class="bg-red-500/10 border border-red-500/20 rounded-xl px-5 py-3 flex items-center justify-between">
          <span class="text-red-600 text-sm">{error}</span>
          <button class="text-red-600 hover:text-red-500" onclick={() => error = null}>
            <span class="material-symbols-outlined text-[20px]">close</span>
          </button>
        </div>
      {/if}

      <!-- URL Input -->
      <div class="mt-4">
        <div class="bg-yt-highlight border border-gray-200 rounded-xl p-1 shadow-lg shadow-black/5">
          <div class="flex flex-col lg:flex-row gap-2">
            <div class="flex-1 relative group">
              <div class="absolute inset-y-0 left-4 flex items-center pointer-events-none text-gray-400 group-focus-within:text-yt-primary transition-colors">
                <span class="material-symbols-outlined">link</span>
              </div>
              <input
                class="w-full h-10 bg-yt-surface text-gray-900 rounded-xl pl-12 pr-4 border border-gray-200 focus:ring-2 focus:ring-yt-primary focus:outline-none placeholder-gray-400 font-mono text-sm"
                placeholder="Paste YouTube, Twitch, or video URL here..."
                type="text"
                bind:value={url}
                onkeydown={handleKeydown}
                disabled={analyzing || downloading}
              />
            </div>
            <div class="flex gap-2">
              <button
                class="h-10 px-6 rounded-xl bg-yt-surface hover:bg-gray-100 text-gray-600 font-medium flex items-center gap-2 transition-colors border border-gray-200"
                onclick={handlePaste}
                disabled={analyzing || downloading}
              >
                <span class="material-symbols-outlined text-[20px]">content_paste</span>
                <span>Paste</span>
              </button>
              <button
                class="h-10 px-6 rounded-xl bg-yt-primary hover:bg-blue-600 text-white font-bold flex items-center gap-2 transition-all shadow-lg shadow-yt-primary/20 disabled:opacity-50"
                onclick={handleAnalyze}
                disabled={analyzing || downloading || !url.trim()}
              >
                {#if analyzing}
                  <span class="material-symbols-outlined text-[20px] animate-spin">progress_activity</span>
                {:else}
                  <span class="material-symbols-outlined text-[20px]">search</span>
                {/if}
                <span>Analyze</span>
              </button>
            </div>
          </div>
        </div>
      </div>

      <!-- Video Info Banner (after analyze) -->
      {#if videoInfo}
        <div class="bg-yt-highlight rounded-xl p-4 flex items-center gap-4 border border-gray-200">
          {#if videoInfo.thumbnail}
            <img src={videoInfo.thumbnail} alt="" class="w-32 h-20 rounded-xl object-cover shrink-0" />
          {/if}
          <div class="flex-1 min-w-0">
            <h3 class="font-display font-semibold text-gray-900 truncate">{videoInfo.title}</h3>
            <p class="text-gray-500 text-sm mt-1">{videoInfo.channel} &middot; {formatDuration(videoInfo.duration)}</p>
          </div>
        </div>
      {/if}

      <!-- Playlist / Channel Result -->
      {#if playlistResult}
        <div class="bg-yt-highlight rounded-xl border border-gray-200 overflow-hidden">
          <!-- Playlist Header -->
          <div class="p-5 border-b border-gray-200">
            <div class="flex items-center gap-3">
              <div class="p-2 bg-yt-primary/10 rounded-lg text-yt-primary">
                <span class="material-symbols-outlined text-[24px]">playlist_play</span>
              </div>
              <div class="flex-1 min-w-0">
                <h3 class="font-display font-semibold text-gray-900 truncate">{playlistResult.title}</h3>
                <p class="text-gray-500 text-sm mt-0.5">
                  {#if playlistResult.channelName}{playlistResult.channelName} &middot; {/if}{playlistResult.videoCount ?? playlistResult.entries.length}개 영상
                </p>
              </div>
            </div>
          </div>

          <!-- Video List -->
          <div class="max-h-[400px] overflow-y-auto hide-scrollbar divide-y divide-gray-100">
            {#each playlistResult.entries as entry, i}
              <button
                class="w-full flex items-center gap-3 p-3 hover:bg-gray-50 transition-colors text-left disabled:opacity-50"
                onclick={() => handleSelectVideo(entry)}
                disabled={analyzing}
              >
                <span class="text-gray-400 text-xs font-mono w-6 text-right shrink-0">{i + 1}</span>
                {#if entry.thumbnail}
                  <img src={entry.thumbnail} alt="" class="w-20 h-12 rounded-lg object-cover shrink-0 bg-gray-100" />
                {:else}
                  <div class="w-20 h-12 rounded-lg bg-gray-100 shrink-0 flex items-center justify-center">
                    <span class="material-symbols-outlined text-gray-300 text-[20px]">movie</span>
                  </div>
                {/if}
                <div class="flex-1 min-w-0">
                  <p class="text-sm text-gray-900 truncate">{entry.title || "제목 없음"}</p>
                  {#if entry.duration}
                    <p class="text-xs text-gray-400 mt-0.5">{formatDuration(entry.duration)}</p>
                  {/if}
                </div>
                <span class="material-symbols-outlined text-gray-400 text-[18px] shrink-0 opacity-0 group-hover:opacity-100">arrow_forward</span>
              </button>
            {/each}
          </div>

          <!-- Load More -->
          {#if playlistResult.videoCount && playlistResult.entries.length < playlistResult.videoCount}
            <div class="p-3 border-t border-gray-200">
              <button
                class="w-full py-2.5 rounded-xl bg-yt-surface hover:bg-gray-100 text-gray-600 text-sm font-medium transition-colors flex items-center justify-center gap-2 disabled:opacity-50 border border-gray-200"
                onclick={handleLoadMore}
                disabled={loadingMore}
              >
                {#if loadingMore}
                  <span class="material-symbols-outlined text-[18px] animate-spin">progress_activity</span>
                  불러오는 중...
                {:else}
                  <span class="material-symbols-outlined text-[18px]">expand_more</span>
                  더 보기 ({playlistResult.entries.length} / {playlistResult.videoCount})
                {/if}
              </button>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Format / Quality / Subtitles Cards -->
      <div class="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <!-- Format -->
        <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
          <div class="flex items-center gap-3 mb-3">
            <div class="p-2 bg-purple-500/10 rounded-lg text-purple-600">
              <span class="material-symbols-outlined text-[20px]">movie</span>
            </div>
            <h3 class="font-display font-semibold text-base text-gray-900">Format</h3>
          </div>
          <div class="grid grid-cols-3 gap-2 bg-yt-surface p-1 rounded-xl border border-gray-200">
            <button
              class="py-2 rounded-lg text-sm font-medium transition-colors {format === 'mp4' ? 'bg-yt-primary text-white shadow-sm' : 'text-gray-500 hover:text-gray-900 hover:bg-gray-100'}"
              onclick={() => format = "mp4"}
            >MP4</button>
            <button
              class="py-2 rounded-lg text-sm font-medium transition-colors {format === 'mkv' ? 'bg-yt-primary text-white shadow-sm' : 'text-gray-500 hover:text-gray-900 hover:bg-gray-100'}"
              onclick={() => format = "mkv"}
            >MKV</button>
            <button
              class="py-2 rounded-lg text-sm font-medium transition-colors {format === 'mp3' ? 'bg-yt-primary text-white shadow-sm' : 'text-gray-500 hover:text-gray-900 hover:bg-gray-100'}"
              onclick={() => format = "mp3"}
            >MP3</button>
          </div>
        </div>

        <!-- Quality -->
        <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
          <div class="flex items-center gap-3 mb-3">
            <div class="p-2 bg-amber-500/10 rounded-lg text-amber-600">
              <span class="material-symbols-outlined text-[20px]">hd</span>
            </div>
            <h3 class="font-display font-semibold text-base text-gray-900">Quality</h3>
          </div>
          <div class="relative">
            <select
              class="w-full bg-yt-surface text-gray-900 border border-gray-200 rounded-xl px-4 py-2.5 focus:ring-2 focus:ring-yt-primary focus:outline-none appearance-none cursor-pointer"
              bind:value={quality}
              disabled={format === "mp3"}
            >
              <option value="best">Best Available (4K/8K)</option>
              <option value="1080p">1080p (Full HD)</option>
              <option value="720p">720p (HD)</option>
              <option value="480p">480p</option>
            </select>
            <div class="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-gray-400">
              <span class="material-symbols-outlined text-[20px]">expand_more</span>
            </div>
          </div>
        </div>

        <!-- Subtitles -->
        <div class="bg-yt-highlight rounded-xl p-4 border border-gray-200">
          <div class="flex items-center gap-3 mb-3">
            <div class="p-2 bg-emerald-500/10 rounded-lg text-emerald-600">
              <span class="material-symbols-outlined text-[20px]">subtitles</span>
            </div>
            <h3 class="font-display font-semibold text-base text-gray-900">Subtitles</h3>
          </div>
          <div class="flex items-center justify-between bg-yt-surface p-2.5 rounded-xl px-4 border border-gray-200">
            <span class="text-sm text-gray-600">Embed Subs</span>
            <label class="relative inline-flex items-center cursor-pointer">
              <input type="checkbox" bind:checked={embedSubs} class="sr-only peer" />
              <div class="w-9 h-5 bg-gray-300 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-yt-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-yt-primary"></div>
            </label>
          </div>
        </div>
      </div>

      <!-- Download Progress -->
      {#if downloading || downloadStatus === "completed" || downloadStatus === "failed"}
        <div class="bg-yt-highlight rounded-xl p-4 border {downloading ? 'border-yt-primary/30' : downloadStatus === 'completed' ? 'border-green-500/30' : 'border-red-500/30'} relative overflow-hidden">
          {#if downloading}
            <div class="absolute bottom-0 left-0 h-1 bg-yt-primary transition-all" style="width: {progress}%"></div>
          {/if}
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-4">
              {#if downloading}
                <span class="material-symbols-outlined text-yt-primary animate-spin">progress_activity</span>
              {:else if downloadStatus === "completed"}
                <span class="material-symbols-outlined text-green-600">check_circle</span>
              {:else}
                <span class="material-symbols-outlined text-red-600">error</span>
              {/if}
              <div>
                <p class="text-gray-900 font-medium">
                  {#if downloading}다운로드 중... {progress.toFixed(0)}%{:else if downloadStatus === "completed"}다운로드 완료!{:else}다운로드 실패{/if}
                </p>
                {#if downloading && speed}
                  <p class="text-gray-500 text-sm">{speed} &middot; ETA: {eta}</p>
                {/if}
              </div>
            </div>
            {#if downloading}
              <button class="text-gray-400 hover:text-red-600 transition-colors" onclick={handleCancelDownload}>
                <span class="material-symbols-outlined">close</span>
              </button>
            {/if}
          </div>
        </div>
      {/if}

      <!-- Start Download Button -->
      <div class="mt-4">
        <button
          class="w-full group relative overflow-hidden rounded-xl bg-gradient-to-r from-yt-primary to-blue-600 p-[1px] focus:outline-none focus:ring-2 focus:ring-yt-primary focus:ring-offset-2 focus:ring-offset-yt-bg disabled:opacity-50 disabled:cursor-not-allowed"
          onclick={handleStartDownload}
          disabled={downloading || (!videoInfo && !url.trim())}
        >
          <div class="relative h-11 bg-yt-surface group-hover:bg-opacity-0 transition-all rounded-xl flex items-center justify-center gap-3">
            <div class="absolute inset-0 bg-gradient-to-r from-yt-primary to-blue-600 opacity-20 group-hover:opacity-100 transition-opacity duration-300 rounded-xl"></div>
            {#if downloading}
              <span class="material-symbols-outlined text-white text-[20px] z-10 animate-spin">progress_activity</span>
              <span class="text-sm font-semibold text-white z-10 font-display tracking-wide">Downloading...</span>
            {:else}
              <span class="material-symbols-outlined text-white text-[20px] z-10">download</span>
              <span class="text-sm font-semibold text-white z-10 font-display tracking-wide">Start Download</span>
            {/if}
          </div>
        </button>
        <p class="text-center text-gray-400 text-sm mt-3">
          Downloads are saved to <span class="text-gray-600 font-mono">{downloadPath}</span>
        </p>
      </div>
    </div>
</div>
