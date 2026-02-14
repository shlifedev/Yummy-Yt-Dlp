<script lang="ts">
  import { commands, type PlaylistResult, type DuplicateCheckResult } from "$lib/bindings"
  import { listen } from "@tauri-apps/api/event"
  import { onMount, onDestroy } from "svelte"
  import { t } from "$lib/i18n/index.svelte"

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

  // Filename template state
  let filenameExpanded = $state(true)
  let useAdvancedTemplate = $state(false)
  let filenameTemplate = $state("%(title)s.%(ext)s")
  let templateUploaderFolder = $state(false)
  let templateUploadDate = $state(false)
  let templateVideoId = $state(false)
  let fullSettings = $state<any>(null)
  let savingTemplate = $state(false)

  // Download state
  let downloading = $state(false)
  let downloadStatus = $state<string>("idle")
  let progress = $state(0)
  let speed = $state("")
  let eta = $state("")
  let taskId = $state<number | null>(null)

  // Multi-select state
  let selectedEntries = $state<Set<string>>(new Set())
  let allSelected = $derived(
    playlistResult ? selectedEntries.size === playlistResult.entries.length && playlistResult.entries.length > 0 : false
  )

  // Batch download state
  let downloadingAll = $state(false)
  let batchProgress = $state({ current: 0, total: 0 })

  // Duplicate check state
  let duplicateCheck = $state<DuplicateCheckResult | null>(null)
  let pendingRequest = $state<any>(null)

  // Analyze elapsed time
  let analyzeStartTime = $state<number | null>(null)
  let analyzeElapsed = $state(0)
  let analyzeTimerInterval: ReturnType<typeof setInterval> | null = null

  // Cancel generation counter
  let analyzeGeneration = $state(0)

  // "Load more" end-of-list flag
  let noMoreEntries = $state(false)

  // Auto-analyze (NOT $state to avoid being tracked by $effect)
  let analyzeTimeoutId: ReturnType<typeof setTimeout> | null = null

  function looksLikeVideoUrl(value: string): boolean {
    return /^https?:\/\/.+/.test(value.trim())
  }

  // Settings
  let downloadPath = $state("~/Downloads")

  let unlisten: (() => void) | null = null

  $effect(() => {
    const currentUrl = url // Only tracked dependency
    if (analyzeTimeoutId) {
      clearTimeout(analyzeTimeoutId)
      analyzeTimeoutId = null
    }
    if (looksLikeVideoUrl(currentUrl)) {
      analyzeTimeoutId = setTimeout(() => {
        // Read analyzing/downloading at execution time, not tracked by $effect
        if (!analyzing && !downloading) {
          handleAnalyze()
        }
      }, 800)
    }
    return () => {
      if (analyzeTimeoutId) {
        clearTimeout(analyzeTimeoutId)
        analyzeTimeoutId = null
      }
    }
  })

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
    stopAnalyzeTimer()
  })

  async function loadSettings() {
    try {
      const result = await commands.getSettings()
      if (result.status === "ok") {
        fullSettings = result.data
        downloadPath = result.data.downloadPath
        useAdvancedTemplate = result.data.useAdvancedTemplate
        filenameTemplate = result.data.filenameTemplate
        templateUploaderFolder = result.data.templateUploaderFolder
        templateUploadDate = result.data.templateUploadDate
        templateVideoId = result.data.templateVideoId
      }
    } catch (e) { console.error("Failed to load settings:", e) }
  }

  function buildTemplate(): string {
    let name = "%(title)s"
    if (templateUploadDate) name = "%(upload_date)s " + name
    if (templateVideoId) name = name + " [%(id)s]"
    let path = name + ".%(ext)s"
    if (templateUploaderFolder) path = "%(uploader)s/" + path
    return path
  }

  function getTemplatePreview(): string {
    if (useAdvancedTemplate) return filenameTemplate
    let name = "Title"
    if (templateUploadDate) name = "20240101 " + name
    if (templateVideoId) name = name + " [dQw4w9WgXcQ]"
    let path = name + ".mp4"
    if (templateUploaderFolder) path = "Uploader/" + path
    return path
  }

  async function saveTemplateSettings() {
    if (!fullSettings || savingTemplate) return
    savingTemplate = true
    try {
      const updated = {
        ...fullSettings,
        useAdvancedTemplate,
        filenameTemplate: useAdvancedTemplate ? filenameTemplate : buildTemplate(),
        templateUploaderFolder,
        templateUploadDate,
        templateVideoId,
      }
      await commands.updateSettings(updated)
      fullSettings = updated
    } catch (e) { console.error("Failed to save template settings:", e) }
    finally { savingTemplate = false }
  }

  function startAnalyzeTimer() {
    analyzeStartTime = Date.now()
    analyzeElapsed = 0
    analyzeTimerInterval = setInterval(() => {
      if (analyzeStartTime) analyzeElapsed = Math.floor((Date.now() - analyzeStartTime) / 1000)
    }, 1000)
  }

  function stopAnalyzeTimer() {
    if (analyzeTimerInterval) { clearInterval(analyzeTimerInterval); analyzeTimerInterval = null }
    analyzeStartTime = null
    analyzeElapsed = 0
  }

  function handleCancelAnalyze() {
    analyzeGeneration++ // invalidate in-flight requests
    analyzing = false
    stopAnalyzeTimer()
  }

  function extractError(err: any): string {
    if (typeof err === "string") return err
    const values = Object.values(err)
    return (values[0] as string) || "알 수 없는 오류"
  }

  async function handleAnalyze() {
    if (!url.trim()) return
    analyzing = true
    error = null
    videoInfo = null
    playlistResult = null
    playlistPage = 0
    selectedEntries = new Set()
    noMoreEntries = false
    const currentGeneration = ++analyzeGeneration
    startAnalyzeTimer()

    try {
      const valResult = await commands.validateUrl(url)
      if (currentGeneration !== analyzeGeneration) return
      if (valResult.status === "error") {
        error = extractError(valResult.error)
        return
      }
      if (!valResult.data.valid) {
        error = t("download.invalidUrl")
        return
      }

      const normalized = valResult.data.normalizedUrl || url

      if (valResult.data.urlType === "video") {
        const infoResult = await commands.fetchVideoInfo(normalized)
        if (currentGeneration !== analyzeGeneration) return
        if (infoResult.status === "error") {
          error = extractError(infoResult.error)
          return
        }
        videoInfo = infoResult.data
      } else if (valResult.data.urlType === "channel" || valResult.data.urlType === "playlist") {
        const plResult = await commands.fetchPlaylistInfo(normalized, 0, 50)
        if (currentGeneration !== analyzeGeneration) return
        if (plResult.status === "error") {
          error = extractError(plResult.error)
          return
        }
        playlistResult = plResult.data
      }
    } catch (e: any) {
      if (currentGeneration !== analyzeGeneration) return
      error = e.message || String(e)
    } finally {
      if (currentGeneration === analyzeGeneration) {
        analyzing = false
        stopAnalyzeTimer()
      }
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
      } else {
        noMoreEntries = true
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
    error = null
    duplicateCheck = null
    pendingRequest = null

    const request = {
      videoUrl: videoInfo?.url || url,
      videoId: videoInfo?.videoId || "",
      title: videoInfo?.title || url,
      formatId: buildFormatString(),
      qualityLabel: quality === "best" ? "Best" : quality,
      outputDir: null,
      cookieBrowser: null,
    }

    // Check for duplicates if we have a video ID
    if (request.videoId) {
      try {
        const dupResult = await commands.checkDuplicate(request.videoId)
        if (dupResult.status === "ok" && dupResult.data) {
          if (dupResult.data.inQueue) {
            error = t("download.alreadyInQueue")
            return
          }
          if (dupResult.data.inHistory) {
            duplicateCheck = dupResult.data
            pendingRequest = request
            return
          }
        }
      } catch (e) {
        // Duplicate check failed, proceed with download anyway
      }
    }

    await executeDownload(request)
  }

  async function executeDownload(request: any) {
    downloading = true
    downloadStatus = "downloading"
    progress = 0
    speed = ""
    eta = ""
    error = null
    duplicateCheck = null
    pendingRequest = null

    try {
      const result = await commands.addToQueue(request)
      if (result.status === "error") {
        downloadStatus = "failed"
        downloading = false
        error = extractError(result.error)
      } else {
        window.dispatchEvent(new CustomEvent("queue-added", { detail: { count: 1 } }))
        downloading = false
        downloadStatus = "idle"
        url = ""
        videoInfo = null
        playlistResult = null
        taskId = null
      }
    } catch (e: any) {
      downloadStatus = "failed"
      downloading = false
      error = e.message || String(e)
    }
  }

  function confirmDuplicate() {
    if (pendingRequest) executeDownload(pendingRequest)
  }

  function cancelDuplicate() {
    duplicateCheck = null
    pendingRequest = null
  }

  async function handleCancelDownload() {
    if (taskId) {
      try {
        await commands.cancelDownload(taskId)
        downloadStatus = "cancelled"
        downloading = false
      } catch (e) { console.error("Failed to cancel download:", e) }
    }
  }

  function toggleSelect(videoId: string) {
    const next = new Set(selectedEntries)
    if (next.has(videoId)) {
      next.delete(videoId)
    } else {
      next.add(videoId)
    }
    selectedEntries = next
  }

  function toggleSelectAll() {
    if (!playlistResult) return
    if (allSelected) {
      selectedEntries = new Set()
    } else {
      selectedEntries = new Set(playlistResult.entries.map(e => e.videoId))
    }
  }

  async function handleDownloadSelected() {
    if (!playlistResult || downloadingAll || selectedEntries.size === 0) return
    downloadingAll = true
    error = null

    try {
      const entries = playlistResult.entries.filter(e => selectedEntries.has(e.videoId))
      const totalCount = entries.length
      batchProgress = { current: 0, total: totalCount }
      const formatStr = buildFormatString()
      const qualityLabel = quality === "best" ? "Best" : quality
      let skippedQueue = 0

      let queued = 0
      for (const entry of entries) {
        if (!downloadingAll) break

        // Skip if already in queue
        try {
          const dupResult = await commands.checkDuplicate(entry.videoId)
          if (!downloadingAll) break
          if (dupResult.status === "ok" && dupResult.data?.inQueue) {
            skippedQueue++
            batchProgress = { current: batchProgress.current + 1, total: totalCount }
            continue
          }
        } catch (e) { /* proceed on error */ }

        if (!downloadingAll) break

        const request = {
          videoUrl: entry.url,
          videoId: entry.videoId,
          title: entry.title || `Video ${entry.videoId}`,
          formatId: formatStr,
          qualityLabel,
          outputDir: null,
          cookieBrowser: null,
        }

        const result = await commands.addToQueue(request)
        if (!downloadingAll) break
        if (result.status === "error") {
          console.error(`Failed to queue ${entry.title}:`, extractError(result.error))
        } else {
          queued++
        }

        batchProgress = { current: batchProgress.current + 1, total: totalCount }
      }

      if (skippedQueue > 0) {
        error = t("download.skippedQueue", { count: skippedQueue })
      }
      if (queued > 0) {
        window.dispatchEvent(new CustomEvent("queue-added", { detail: { count: queued } }))
      }
      url = ""
      videoInfo = null
      playlistResult = null
      selectedEntries = new Set()
    } catch (e: any) {
      error = e.message || String(e)
    } finally {
      downloadingAll = false
      batchProgress = { current: 0, total: 0 }
    }
  }

  async function handleDownloadAll() {
    if (!playlistResult || downloadingAll) return
    downloadingAll = true
    error = null

    try {
      // Fetch all entries if not fully loaded
      let allEntries = playlistResult.entries
      if (playlistResult.videoCount == null || allEntries.length < (playlistResult.videoCount ?? Infinity)) {
        const fullResult = await commands.fetchPlaylistInfo(playlistResult.url, 0, 99999)
        if (fullResult.status === "error") {
          error = extractError(fullResult.error)
          return
        }
        allEntries = fullResult.data.entries
      }

      const totalCount = allEntries.length
      batchProgress = { current: 0, total: totalCount }
      const formatStr = buildFormatString()
      const qualityLabel = quality === "best" ? "Best" : quality
      let skippedQueue = 0

      let queued = 0
      for (const entry of allEntries) {
        if (!downloadingAll) break // cancelled by user

        // Skip if already in queue
        try {
          const dupResult = await commands.checkDuplicate(entry.videoId)
          if (!downloadingAll) break
          if (dupResult.status === "ok" && dupResult.data?.inQueue) {
            skippedQueue++
            batchProgress = { current: batchProgress.current + 1, total: totalCount }
            continue
          }
        } catch (e) { /* proceed on error */ }

        if (!downloadingAll) break

        const request = {
          videoUrl: entry.url,
          videoId: entry.videoId,
          title: entry.title || `Video ${entry.videoId}`,
          formatId: formatStr,
          qualityLabel,
          outputDir: null,
          cookieBrowser: null,
        }

        const result = await commands.addToQueue(request)
        if (!downloadingAll) break
        if (result.status === "error") {
          console.error(`Failed to queue ${entry.title}:`, extractError(result.error))
        } else {
          queued++
        }

        batchProgress = { current: batchProgress.current + 1, total: totalCount }
      }

      if (skippedQueue > 0) {
        error = t("download.skippedQueue", { count: skippedQueue })
      }
      if (queued > 0) {
        window.dispatchEvent(new CustomEvent("queue-added", { detail: { count: queued } }))
      }
      url = ""
      videoInfo = null
      playlistResult = null
    } catch (e: any) {
      error = e.message || String(e)
    } finally {
      downloadingAll = false
      batchProgress = { current: 0, total: 0 }
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !downloading) handleAnalyze()
  }

  function formatDuration(seconds: number | null | undefined): string {
    if (seconds === null || seconds === undefined || isNaN(seconds)) return "--:--"
    const m = Math.floor(seconds / 60)
    const s = seconds % 60
    return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
  }

  // 5-3: Fix formatSize(0) returning empty string
  function formatSize(bytes: number | null): string {
    if (bytes === null || bytes === undefined) return ""
    if (bytes === 0) return "0 MB"
    const mb = bytes / (1024 ** 2)
    if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`
    return `${Math.round(mb)} MB`
  }
</script>

<div class="h-full overflow-y-auto hide-scrollbar">
    <div class="px-5 py-3 pb-5 space-y-3">
      <!-- Error -->
      {#if error}
        <div class="bg-red-500/10 rounded-lg px-4 py-2 flex items-center justify-between">
          <span class="text-red-400 text-xs">{error}</span>
          <button class="text-red-400 hover:text-red-500" onclick={() => error = null}>
            <span class="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>
      {/if}

      <!-- Duplicate Warning -->
      {#if duplicateCheck}
        <div class="bg-amber-500/10 rounded-lg px-4 py-3 flex items-center justify-between gap-3">
          <div class="flex items-center gap-2 min-w-0">
            <span class="material-symbols-outlined text-amber-400 text-[20px] shrink-0">warning</span>
            <span class="text-amber-400 text-xs truncate">
              {t("download.alreadyDownloaded", { title: videoInfo?.title || pendingRequest?.title || "" })}
            </span>
          </div>
          <div class="flex items-center gap-2 shrink-0">
            <button
              class="px-3 py-1.5 rounded-md bg-amber-500 hover:bg-amber-600 text-white text-xs font-medium transition-colors"
              onclick={confirmDuplicate}
            >{t("download.redownload")}</button>
            <button
              class="px-3 py-1.5 rounded-md bg-white/[0.04] hover:bg-white/[0.08] text-gray-400 text-xs font-medium transition-colors"
              onclick={cancelDuplicate}
            >{t("download.cancel")}</button>
          </div>
        </div>
      {/if}

      <!-- Analyzing Indicator -->
      {#if analyzing}
        <div class="bg-blue-500/10 rounded-lg px-4 py-2 flex items-center justify-between">
          <div class="flex items-center gap-2">
            <span class="material-symbols-outlined text-blue-500 text-[18px] animate-spin">progress_activity</span>
            <span class="text-blue-400 text-xs font-medium">
              {t("download.analyzing")}
              {#if analyzeElapsed > 0}({analyzeElapsed}초){/if}
            </span>
          </div>
          <button class="text-blue-400 hover:text-blue-300 transition-colors" onclick={handleCancelAnalyze}>
            <span class="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>
      {/if}

      <!-- URL Input -->
      <div class="flex flex-row gap-2">
        <div class="flex-1 relative group">
          <div class="absolute inset-y-0 left-4 flex items-center pointer-events-none text-gray-500 group-focus-within:text-yt-primary transition-colors">
            <span class="material-symbols-outlined text-[20px]">link</span>
          </div>
          <input
            class="w-full h-10 bg-yt-surface text-gray-100 rounded-lg pl-11 pr-4 border border-white/[0.06] focus:ring-2 focus:ring-yt-primary focus:outline-none placeholder-gray-600 font-mono text-sm"
            placeholder={t("download.urlPlaceholder")}
            type="text"
            bind:value={url}
            onkeydown={handleKeydown}
            disabled={downloading}
          />
        </div>
        <button
          class="h-10 px-5 rounded-lg shrink-0 bg-yt-primary hover:bg-blue-500 text-white font-bold flex items-center gap-2 transition-all disabled:opacity-50 text-sm"
          onclick={playlistResult && !videoInfo
            ? (selectedEntries.size > 0 ? handleDownloadSelected : handleDownloadAll)
            : handleStartDownload}
          disabled={downloading || downloadingAll || (!videoInfo && !playlistResult && !url.trim())}
        >
          <span class="material-symbols-outlined text-[18px]">download</span>
          {#if downloadingAll}
            <span>{t("download.queuing")} ({batchProgress.current}/{batchProgress.total})</span>
          {:else if playlistResult && !videoInfo && selectedEntries.size > 0}
            <span>{t("download.downloadSelected")} ({selectedEntries.size})</span>
          {:else if playlistResult && !videoInfo}
            <span>{t("download.downloadAll")} ({playlistResult.videoCount ?? playlistResult.entries.length})</span>
          {:else}
            <span>{t("download.download")}</span>
          {/if}
        </button>
      </div>

      <!-- Video Info Banner (after analyze) -->
      {#if videoInfo}
        <div class="pb-3 border-b border-white/[0.06]">
          {#if videoInfo && playlistResult}
            <button
              class="text-sm text-yt-primary hover:text-blue-400 flex items-center gap-1 transition-colors mb-2"
              onclick={() => { videoInfo = null }}
            >
              <span class="material-symbols-outlined text-[16px]">arrow_back</span>
              {t("download.backToPlaylist")} ({t("download.videos", { count: playlistResult.videoCount ?? playlistResult.entries.length })})
            </button>
          {/if}
          <div class="flex items-center gap-3">
            {#if videoInfo.thumbnail}
              <img src={videoInfo.thumbnail} alt="" class="w-24 h-16 rounded-lg object-cover shrink-0" />
            {/if}
            <div class="flex-1 min-w-0">
              <h3 class="font-display font-semibold text-gray-100 truncate text-sm">{videoInfo.title}</h3>
              <p class="text-gray-400 text-xs mt-0.5">{videoInfo.channel || "Unknown"} &middot; {formatDuration(videoInfo.duration)}</p>
            </div>
          </div>
        </div>
      {/if}

      <!-- Playlist / Channel Result -->
      {#if playlistResult}
        <div class="border border-white/[0.06] rounded-lg overflow-hidden">
          <!-- Playlist Header -->
          <div class="px-3 py-3 border-b border-white/[0.04]">
            <div class="flex items-center gap-3">
              <span class="material-symbols-outlined text-[20px] text-yt-primary">playlist_play</span>
              <div class="flex-1 min-w-0">
                <h3 class="font-display font-semibold text-sm text-gray-100 truncate">{playlistResult.title}</h3>
                <p class="text-gray-400 text-xs mt-0.5">
                  {#if playlistResult.channelName}{playlistResult.channelName} &middot; {/if}{t("download.videos", { count: playlistResult.videoCount ?? playlistResult.entries.length })}
                </p>
              </div>
              <div class="flex items-center gap-1.5 shrink-0">
                <button
                  class="px-2.5 py-1.5 rounded-md bg-white/[0.04] hover:bg-white/[0.06] text-gray-400 text-xs font-medium transition-colors flex items-center gap-1"
                  onclick={toggleSelectAll}
                >
                  <span class="material-symbols-outlined text-[16px]">{allSelected ? "deselect" : "select_all"}</span>
                  {allSelected ? t("download.deselect") : t("download.selectAll")}
                </button>
                <button
                  class="px-3 py-1.5 rounded-md bg-yt-primary hover:bg-blue-500 text-white text-xs font-medium transition-all flex items-center gap-1.5 disabled:opacity-50"
                  onclick={selectedEntries.size > 0 ? handleDownloadSelected : handleDownloadAll}
                  disabled={downloadingAll || downloading}
                >
                  <span class="material-symbols-outlined text-[16px]">playlist_add</span>
                  {#if downloadingAll}
                    {batchProgress.current}/{batchProgress.total}
                  {:else if selectedEntries.size > 0}
                    {t("download.download")} ({selectedEntries.size})
                  {:else}
                    {t("download.downloadAll")}
                  {/if}
                </button>
              </div>
            </div>
            {#if downloadingAll}
              <div class="mt-2">
                <div class="flex items-center gap-2 mb-1">
                  <span class="material-symbols-outlined text-yt-primary animate-spin text-[16px]">progress_activity</span>
                  <span class="text-xs text-gray-400">큐에 추가 중... {batchProgress.current} / {batchProgress.total}</span>
                  <button class="ml-auto text-xs text-gray-500 hover:text-red-400 transition-colors" onclick={() => downloadingAll = false}>
                    취소
                  </button>
                </div>
                <div class="w-full bg-white/[0.06] rounded-full h-1">
                  <div class="bg-yt-primary h-1 rounded-full transition-all" style="width: {batchProgress.total > 0 ? (batchProgress.current / batchProgress.total * 100) : 0}%"></div>
                </div>
              </div>
            {/if}
          </div>

          <!-- Video List -->
          <div class="max-h-[400px] overflow-y-auto hide-scrollbar divide-y divide-white/[0.04]">
            {#each playlistResult.entries as entry, i}
              <div
                class="w-full flex items-center gap-3 p-3 hover:bg-white/[0.03] transition-colors {selectedEntries.has(entry.videoId) ? 'bg-yt-primary/5' : ''}"
              >
                <button
                  type="button"
                  class="shrink-0 flex items-center p-0 bg-transparent border-none"
                  aria-label="Select {entry.title || 'video'}"
                  onclick={(e: MouseEvent) => { e.stopPropagation(); toggleSelect(entry.videoId) }}
                >
                  <input
                    type="checkbox"
                    checked={selectedEntries.has(entry.videoId)}
                    class="w-4 h-4 rounded border-white/10 text-yt-primary focus:ring-yt-primary cursor-pointer pointer-events-none"
                    tabindex={-1}
                  />
                </button>
                <button
                  class="flex items-center gap-3 flex-1 min-w-0 text-left disabled:opacity-50"
                  onclick={() => handleSelectVideo(entry)}
                  disabled={analyzing}
                >
                  <span class="text-gray-400 text-xs font-mono w-6 text-right shrink-0">{i + 1}</span>
                  {#if entry.thumbnail}
                    <img src={entry.thumbnail} alt="" class="w-20 h-12 rounded-lg object-cover shrink-0 bg-white/[0.04]" />
                  {:else}
                    <div class="w-20 h-12 rounded-lg bg-white/[0.04] shrink-0 flex items-center justify-center">
                      <span class="material-symbols-outlined text-gray-600 text-[20px]">movie</span>
                    </div>
                  {/if}
                  <div class="flex-1 min-w-0">
                    <p class="text-sm text-gray-100 truncate">{entry.title || t("download.noTitle")}</p>
                    {#if entry.duration}
                      <p class="text-xs text-gray-400 mt-0.5">{formatDuration(entry.duration)}</p>
                    {/if}
                  </div>
                  <span class="material-symbols-outlined text-gray-400 text-[18px] shrink-0">arrow_forward</span>
                </button>
              </div>
            {/each}
          </div>

          <!-- Load More -->
          {#if !noMoreEntries && (playlistResult.videoCount == null || playlistResult.entries.length < playlistResult.videoCount)}
            <div class="p-2 border-t border-white/[0.04]">
              <button
                class="w-full py-2 rounded-lg bg-yt-surface hover:bg-white/[0.06] text-gray-400 text-xs font-medium transition-colors flex items-center justify-center gap-1.5 disabled:opacity-50"
                onclick={handleLoadMore}
                disabled={loadingMore}
              >
                {#if loadingMore}
                  <span class="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>
                  {t("download.loading")}
                {:else}
                  <span class="material-symbols-outlined text-[16px]">expand_more</span>
                  {#if playlistResult.videoCount}
                    {t("download.loadMoreCount", { loaded: playlistResult.entries.length, total: playlistResult.videoCount })}
                  {:else}
                    {t("download.loadMoreLoaded", { loaded: playlistResult.entries.length })}
                  {/if}
                {/if}
              </button>
            </div>
          {/if}
        </div>
      {/if}

      <!-- Format / Quality / Subtitles — Inline Rows -->
      <div class="divide-y divide-white/[0.04]">
        <!-- Format -->
        <div class="flex items-center gap-3 py-3">
          <span class="material-symbols-outlined text-[20px] text-purple-600">movie</span>
          <span class="text-sm font-medium text-gray-100 w-20 shrink-0">{t("download.format")}</span>
          <div class="flex gap-1 bg-yt-surface p-0.5 rounded-lg border border-white/[0.06]">
            <button
              class="px-3 py-1.5 rounded-md text-xs font-medium transition-colors {format === 'mp4' ? 'bg-yt-primary text-white shadow-sm' : 'text-gray-400 hover:text-gray-100 hover:bg-white/[0.06]'}"
              onclick={() => format = "mp4"}
            >MP4</button>
            <button
              class="px-3 py-1.5 rounded-md text-xs font-medium transition-colors {format === 'mkv' ? 'bg-yt-primary text-white shadow-sm' : 'text-gray-400 hover:text-gray-100 hover:bg-white/[0.06]'}"
              onclick={() => format = "mkv"}
            >MKV</button>
            <button
              class="px-3 py-1.5 rounded-md text-xs font-medium transition-colors {format === 'mp3' ? 'bg-yt-primary text-white shadow-sm' : 'text-gray-400 hover:text-gray-100 hover:bg-white/[0.06]'}"
              onclick={() => format = "mp3"}
            >MP3</button>
          </div>
        </div>

        <!-- Quality -->
        <div class="flex items-center gap-3 py-3">
          <span class="material-symbols-outlined text-[20px] text-amber-400">hd</span>
          <span class="text-sm font-medium text-gray-100 w-20 shrink-0">{t("download.quality")}</span>
          <div class="relative flex-1">
            <select
              class="w-full bg-yt-surface text-gray-100 text-sm border border-white/[0.06] rounded-lg px-3 py-1.5 focus:ring-2 focus:ring-yt-primary focus:outline-none appearance-none cursor-pointer"
              bind:value={quality}
              disabled={format === "mp3"}
            >
              <option value="best">Best Available</option>
              <option value="1080p">1080p</option>
              <option value="720p">720p</option>
              <option value="480p">480p</option>
            </select>
            <div class="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none text-gray-500">
              <span class="material-symbols-outlined text-[18px]">expand_more</span>
            </div>
          </div>
        </div>

        <!-- Subtitles -->
        <div class="flex items-center gap-3 py-3">
          <span class="material-symbols-outlined text-[20px] text-emerald-600">subtitles</span>
          <span class="text-sm font-medium text-gray-100 w-20 shrink-0">{t("download.subtitles")}</span>
          <div class="flex items-center gap-2 ml-auto">
            <span class="text-xs text-gray-400">{t("download.embed")}</span>
            <label class="relative inline-flex items-center cursor-pointer">
              <input type="checkbox" bind:checked={embedSubs} class="sr-only peer" />
              <div class="w-9 h-5 bg-white/10 peer-focus:outline-none peer-focus:ring-2 peer-focus:ring-yt-primary rounded-full peer peer-checked:after:translate-x-full rtl:peer-checked:after:-translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:start-[2px] after:bg-white after:border-white/10 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-yt-primary"></div>
            </label>
          </div>
        </div>

        <!-- Filename Template -->
        <div class="py-3">
          <div class="flex items-center gap-3">
            <span class="material-symbols-outlined text-[20px] text-violet-600">edit_note</span>
            <span class="text-sm font-medium text-gray-100 w-20 shrink-0">{t("download.filename")}</span>
            <span class="text-xs text-gray-400 font-mono truncate flex-1">{getTemplatePreview()}</span>
            <button
              class="flex items-center gap-1 px-2 py-1 rounded-md text-xs transition-colors {filenameExpanded ? 'bg-violet-500/10 text-violet-400' : 'text-gray-500 hover:bg-white/[0.06] hover:text-gray-400'}"
              onclick={() => filenameExpanded = !filenameExpanded}
            >
              <span class="material-symbols-outlined text-[16px]">{filenameExpanded ? "expand_less" : "tune"}</span>
            </button>
          </div>

          {#if filenameExpanded}
            <div class="mt-2 pl-8 space-y-2">
              <div class="flex items-center justify-between">
                <span class="text-xs text-gray-400">{t("download.mode")}</span>
                <button
                  class="template-chip relative flex items-center gap-1 px-2 py-1 rounded-md text-xs font-medium transition-colors {useAdvancedTemplate ? 'bg-violet-500/10 text-violet-400' : 'bg-white/[0.04] text-gray-400 hover:bg-white/[0.06]'}"
                  onclick={() => { useAdvancedTemplate = !useAdvancedTemplate; saveTemplateSettings() }}
                >
                  <span class="material-symbols-outlined text-[14px]">code</span>
                  {t("download.advanced")}
                  <span class="template-tooltip">{t("download.advancedTooltip")}</span>
                </button>
              </div>

              {#if useAdvancedTemplate}
                <input
                  type="text"
                  class="w-full h-8 bg-yt-surface text-gray-100 rounded-md px-3 border border-white/[0.06] focus:ring-2 focus:ring-yt-primary focus:outline-none text-xs font-mono"
                  bind:value={filenameTemplate}
                  onchange={saveTemplateSettings}
                />
                <p class="text-[10px] text-gray-400">%(title)s, %(id)s, %(ext)s, %(uploader)s, %(upload_date)s</p>
              {:else}
                <div class="flex flex-wrap gap-2">
                  <label class="template-chip relative flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border cursor-pointer transition-colors {templateUploaderFolder ? 'border-violet-500/30 bg-violet-500/10 text-violet-400' : 'border-white/[0.06] bg-yt-surface text-gray-400 hover:bg-white/[0.03]'}">
                    <input type="checkbox" bind:checked={templateUploaderFolder} onchange={saveTemplateSettings} class="sr-only" />
                    <span class="text-xs font-medium">{t("download.uploaderFolder")}</span>
                    <span class="template-tooltip">{t("download.uploaderFolderTooltip")}</span>
                  </label>
                  <label class="template-chip relative flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border cursor-pointer transition-colors {templateUploadDate ? 'border-violet-500/30 bg-violet-500/10 text-violet-400' : 'border-white/[0.06] bg-yt-surface text-gray-400 hover:bg-white/[0.03]'}">
                    <input type="checkbox" bind:checked={templateUploadDate} onchange={saveTemplateSettings} class="sr-only" />
                    <span class="text-xs font-medium">{t("download.uploadDate")}</span>
                    <span class="template-tooltip">{t("download.uploadDateTooltip")}</span>
                  </label>
                  <label class="template-chip relative flex items-center gap-1.5 px-2.5 py-1.5 rounded-md border cursor-pointer transition-colors {templateVideoId ? 'border-violet-500/30 bg-violet-500/10 text-violet-400' : 'border-white/[0.06] bg-yt-surface text-gray-400 hover:bg-white/[0.03]'}">
                    <input type="checkbox" bind:checked={templateVideoId} onchange={saveTemplateSettings} class="sr-only" />
                    <span class="text-xs font-medium">{t("download.videoId")}</span>
                    <span class="template-tooltip">{t("download.videoIdTooltip")}</span>
                  </label>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>

      <!-- Download Progress -->
      {#if downloading || downloadStatus === "completed" || downloadStatus === "failed"}
        <div class="py-3 border-b border-white/[0.04] relative">
          {#if downloading}
            <div class="absolute bottom-0 left-0 h-0.5 bg-yt-primary transition-all" style="width: {progress}%"></div>
          {/if}
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-3">
              {#if downloading}
                <span class="material-symbols-outlined text-yt-primary animate-spin text-[20px]">progress_activity</span>
              {:else if downloadStatus === "completed"}
                <span class="material-symbols-outlined text-green-600 text-[20px]">check_circle</span>
              {:else}
                <span class="material-symbols-outlined text-red-400 text-[20px]">error</span>
              {/if}
              <div>
                <p class="text-sm text-gray-100 font-medium">
                  {#if downloading}{t("download.downloading", { percent: progress.toFixed(0) })}{:else if downloadStatus === "completed"}{t("download.downloadComplete")}{:else}{t("download.downloadFailed")}{/if}
                </p>
                {#if downloading && speed}
                  <p class="text-gray-400 text-xs">{speed} &middot; ETA: {eta}</p>
                {/if}
              </div>
            </div>
            {#if downloading}
              <button class="text-gray-500 hover:text-red-400 transition-colors" onclick={handleCancelDownload}>
                <span class="material-symbols-outlined text-[20px]">close</span>
              </button>
            {/if}
          </div>
        </div>
      {/if}

    </div>
</div>

<style>
  .template-chip .template-tooltip {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    white-space: nowrap;
    background: #2a2a35;
    color: white;
    font-size: 11px;
    padding: 4px 8px;
    border-radius: 6px;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.15s ease;
    transition-delay: 0s;
    z-index: 10;
  }
  .template-chip .template-tooltip::after {
    content: "";
    position: absolute;
    top: 100%;
    left: 50%;
    transform: translateX(-50%);
    border: 4px solid transparent;
    border-top-color: #2a2a35;
  }
  .template-chip:hover .template-tooltip {
    opacity: 1;
    transition-delay: 0.5s;
  }
</style>
