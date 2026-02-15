<script lang="ts">
  import { commands, type PlaylistResult, type DuplicateCheckResult, type QuickMetadata } from "$lib/bindings"
  import { listen } from "@tauri-apps/api/event"
  import { platform } from "@tauri-apps/plugin-os"
  import { onMount, onDestroy } from "svelte"
  import { t } from "$lib/i18n/index.svelte"
  import { extractError } from "$lib/utils/errors"
  import { formatSize, formatDuration } from "$lib/utils/format"

  // URL & analyze state
  let url = $state("")
  let analyzing = $state(false)
  let videoInfo = $state<any>(null)
  let quickInfo = $state<QuickMetadata | null>(null)
  let loadingFormats = $state(false)
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

  // Tooltip state (appended to document.body to escape stacking context)
  let tooltipEl: HTMLDivElement | null = null
  let tooltipTimerId: ReturnType<typeof setTimeout> | null = null

  function ensureTooltipEl() {
    if (!tooltipEl) {
      tooltipEl = document.createElement("div")
      tooltipEl.style.cssText = "position:fixed;z-index:999999;pointer-events:none;opacity:0;transform:translate(-50%,-100%) translateY(4px);transition:opacity 0.2s ease,transform 0.2s cubic-bezier(0.16,1,0.3,1);"

      const bubble = document.createElement("div")
      bubble.style.cssText = "background:rgba(25,25,25,0.95);color:#fff;font-size:13px;font-weight:500;line-height:1.5;padding:8px 16px;border-radius:10px;white-space:nowrap;box-shadow:0 4px 20px rgba(0,0,0,0.25),0 1px 6px rgba(0,0,0,0.1);text-align:center;"
      bubble.dataset.role = "text"

      const arrowWrap = document.createElement("div")
      arrowWrap.style.cssText = "display:flex;justify-content:center;"
      const arrow = document.createElement("div")
      arrow.style.cssText = "width:0;height:0;border-left:7px solid transparent;border-right:7px solid transparent;border-top:7px solid rgba(25,25,25,0.95);"
      arrowWrap.appendChild(arrow)

      tooltipEl.appendChild(bubble)
      tooltipEl.appendChild(arrowWrap)
      document.body.appendChild(tooltipEl)
    }
    return tooltipEl
  }

  function showTooltip(e: MouseEvent, text: string) {
    const target = e.currentTarget as HTMLElement
    if (tooltipTimerId) clearTimeout(tooltipTimerId)
    tooltipTimerId = setTimeout(() => {
      const rect = target.getBoundingClientRect()
      const el = ensureTooltipEl()
      const bubble = el.querySelector("[data-role=text]")
      if (bubble) bubble.textContent = text
      el.style.left = rect.left + rect.width / 2 + "px"
      el.style.top = rect.top - 10 + "px"
      el.style.transform = "translate(-50%, -100%) translateY(0px)"
      el.style.opacity = "1"
    }, 500)
  }

  function hideTooltip() {
    if (tooltipTimerId) { clearTimeout(tooltipTimerId); tooltipTimerId = null }
    if (tooltipEl) {
      tooltipEl.style.opacity = "0"
      tooltipEl.style.transform = "translate(-50%, -100%) translateY(4px)"
    }
  }

  // Auto-analyze (NOT $state to avoid being tracked by $effect)
  let analyzeTimeoutId: ReturnType<typeof setTimeout> | null = null

  function looksLikeVideoUrl(value: string): boolean {
    return /^https?:\/\/.+/.test(value.trim())
  }

  // Settings
  let downloadPath = $state("~/Downloads")
  let cookieBrowser = $state<string | null>(null)
  let maxConcurrent = $state(3)
  let browsers = $state<string[]>([])
  let currentPlatform = $state<string>("")

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
    currentPlatform = platform()
    await loadSettings()
    try {
      browsers = await commands.getAvailableBrowsers()
    } catch (e) { console.error("Failed to load browsers:", e) }

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
    if (tooltipTimerId) clearTimeout(tooltipTimerId)
    if (tooltipEl) { tooltipEl.remove(); tooltipEl = null }
  })

  async function loadSettings() {
    try {
      const result = await commands.getSettings()
      if (result.status === "ok") {
        fullSettings = result.data
        downloadPath = result.data.downloadPath
        cookieBrowser = result.data.cookieBrowser
        maxConcurrent = result.data.maxConcurrent
        useAdvancedTemplate = result.data.useAdvancedTemplate
        filenameTemplate = result.data.filenameTemplate
        templateUploaderFolder = result.data.templateUploaderFolder
        templateUploadDate = result.data.templateUploadDate
        templateVideoId = result.data.templateVideoId
      }
    } catch (e) { console.error("Failed to load settings:", e) }
  }

  async function autoSaveSettings(patch: Record<string, any>) {
    if (!fullSettings) return
    const updated = { ...fullSettings, ...patch }
    try {
      await commands.updateSettings(updated)
      fullSettings = updated
    } catch (e) { console.error("Failed to auto-save settings:", e) }
  }

  async function handleSelectDir() {
    try {
      const result = await commands.selectDownloadDirectory()
      if (result.status === "ok" && result.data) {
        downloadPath = result.data
        if (fullSettings) {
          fullSettings.downloadPath = result.data
          await commands.updateSettings(fullSettings)
        }
      }
    } catch (e) { console.error("Failed to select dir:", e) }
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
    quickInfo = null
    loadingFormats = false
    stopAnalyzeTimer()
  }


  async function handleAnalyze() {
    if (!url.trim()) return
    analyzing = true
    error = null
    videoInfo = null
    quickInfo = null
    loadingFormats = false
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
        // Phase 1: Quick metadata via oEmbed (~200ms)
        // Phase 2: Full video info via yt-dlp (~12s) in parallel
        loadingFormats = true

        // Fire both requests in parallel
        const quickPromise = commands.fetchQuickMetadata(normalized)
        const fullPromise = commands.fetchVideoInfo(normalized)

        // Handle quick metadata (Phase 1)
        quickPromise.then((quickResult) => {
          if (currentGeneration !== analyzeGeneration) return
          if (quickResult.status === "ok") {
            quickInfo = quickResult.data
            analyzing = false
            stopAnalyzeTimer()
          }
          // If oEmbed fails, we just wait for yt-dlp (Phase 2)
        }).catch(() => {
          // oEmbed failure is non-fatal
        })

        // Await full video info (Phase 2)
        const infoResult = await fullPromise
        if (currentGeneration !== analyzeGeneration) return
        if (infoResult.status === "error") {
          error = extractError(infoResult.error)
          return
        }
        videoInfo = infoResult.data
        quickInfo = null // Replace quick preview with full info
        loadingFormats = false
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
        loadingFormats = false
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

  async function enqueueBatchDownloads(entries: Array<{ url: string, videoId: string, title: string | null }>) {
    const totalCount = entries.length
    batchProgress = { current: 0, total: totalCount }
    const formatStr = buildFormatString()
    const qualityLabel = quality === "best" ? "Best" : quality
    let skippedQueue = 0
    let queued = 0

    for (const entry of entries) {
      if (!downloadingAll) break

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
  }

  async function handleDownloadSelected() {
    if (!playlistResult || downloadingAll || selectedEntries.size === 0) return
    downloadingAll = true
    error = null

    try {
      const entries = playlistResult.entries.filter(e => selectedEntries.has(e.videoId))
      await enqueueBatchDownloads(entries)
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
      let allEntries = playlistResult.entries
      if (playlistResult.videoCount == null || allEntries.length < (playlistResult.videoCount ?? Infinity)) {
        const fullResult = await commands.fetchPlaylistInfo(playlistResult.url, 0, 99999)
        if (fullResult.status === "error") {
          error = extractError(fullResult.error)
          return
        }
        allEntries = fullResult.data.entries
      }

      await enqueueBatchDownloads(allEntries)
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

</script>

<div class="h-full flex flex-col bg-yt-bg">
    <!-- URL Input Area -->
    <div class="p-6 shrink-0 space-y-4 max-w-3xl mx-auto w-full">
      <!-- Error & Warning -->
      {#if error}
        <div class="bg-yt-error/10 border border-yt-error/20 rounded-lg px-4 py-3 flex items-start gap-3">
          <span class="material-symbols-outlined text-yt-error text-[20px] shrink-0 mt-0.5">error</span>
          <div class="flex-1 min-w-0">
             <p class="text-sm text-yt-text font-medium">{t("download.error")}</p>
             <p class="text-xs text-yt-text-secondary mt-0.5">{error}</p>
          </div>
          <button class="text-yt-text-secondary hover:text-yt-text" aria-label="Close error" onclick={() => error = null}>
            <span class="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>
      {/if}

       {#if duplicateCheck}
        <div class="bg-yt-warning/10 border border-yt-warning/20 rounded-lg px-4 py-3 flex items-start gap-3">
           <span class="material-symbols-outlined text-yt-warning text-[20px] shrink-0 mt-0.5">warning</span>
           <div class="flex-1 min-w-0">
             <p class="text-sm text-yt-text font-medium">{t("download.duplicateFound")}</p>
             <p class="text-xs text-yt-text-secondary mt-0.5">
               {t("download.alreadyDownloaded", { title: videoInfo?.title || pendingRequest?.title || "" })}
            </p>
          </div>
          <div class="flex flex-col gap-2 shrink-0">
             <button
              class="px-3 py-1.5 rounded-md bg-yt-warning hover:bg-yt-warning/80 text-white text-xs font-medium transition-colors"
              onclick={confirmDuplicate}
            >{t("download.redownload")}</button>
            <button
              class="px-3 py-1.5 rounded-md bg-yt-surface hover:bg-yt-highlight border border-yt-border text-yt-text-secondary text-xs font-medium transition-colors"
              onclick={cancelDuplicate}
            >{t("download.cancel")}</button>
          </div>
        </div>
      {/if}
      
      <!-- Input Group -->
      <div class="flex gap-2">
        <div class="flex-1 relative group">
          <div class="absolute inset-y-0 left-3.5 flex items-center pointer-events-none text-yt-text-muted group-focus-within:text-yt-primary transition-colors duration-300">
            <span class="material-symbols-outlined text-[20px] group-focus-within:scale-110 transition-transform">link</span>
          </div>
          <input
            class="w-full h-12 bg-yt-bg text-yt-text rounded-lg pl-11 pr-4 border border-yt-border focus:ring-4 focus:ring-yt-primary/10 focus:border-yt-primary focus:outline-none transition-all duration-300 text-sm placeholder:text-yt-text-muted hover:border-yt-primary/50 shadow-sm"
            placeholder={t("download.urlPlaceholder")}
            type="text"
            bind:value={url}
            onkeydown={handleKeydown}
            disabled={downloading}
          />
           {#if analyzing}
            <div class="absolute inset-y-0 right-3 flex items-center gap-2">
               <span class="material-symbols-outlined text-yt-primary text-[18px] animate-spin">progress_activity</span>
               {#if analyzeElapsed > 0}<span class="text-xs text-yt-text-secondary font-mono">{analyzeElapsed}s</span>{/if}
            </div>
           {/if}
        </div>
        <button
          class="h-12 px-6 rounded-lg shrink-0 bg-yt-primary hover:bg-yt-primary-hover active:scale-95 text-white font-medium flex items-center gap-2 transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed text-sm shadow-md hover:shadow-lg disabled:active:scale-100 disabled:shadow-none"
          onclick={playlistResult && !videoInfo
            ? (selectedEntries.size > 0 ? handleDownloadSelected : handleDownloadAll)
            : handleStartDownload}
          disabled={downloading || downloadingAll || (!videoInfo && !playlistResult && !url.trim())}
        >
          {#if downloadingAll}
            <span class="material-symbols-outlined text-[18px] animate-spin">sync</span>
            <span>{batchProgress.current}/{batchProgress.total}</span>
          {:else if playlistResult && !videoInfo}
             <span class="material-symbols-outlined text-[18px]">playlist_add</span>
             {#if selectedEntries.size > 0}
               <span>{t("download.downloadSelected")} ({selectedEntries.size})</span>
             {:else}
               <span>{t("download.downloadAll")}</span>
             {/if}
          {:else}
            <span class="material-symbols-outlined text-[18px]">download</span>
             <span>{t("download.download")}</span>
          {/if}
        </button>
      </div>

       <!-- Download Options -->
       <div class="mt-4 bg-yt-surface/30 border border-yt-border rounded-lg p-3 space-y-3">
          
          <!-- Top Row: Path & Format -->
          <div class="flex items-center justify-between gap-4">
             <!-- Path -->
             <div class="flex-1 min-w-0 flex items-center gap-2">
                <button 
                  class="flex-1 min-w-0 bg-yt-bg border border-yt-border rounded px-2.5 py-1.5 flex items-center gap-2 relative group hover:bg-yt-highlight hover:border-yt-primary/30 transition-all text-left"
                  onclick={handleSelectDir}
                  title={downloadPath}
                >
                   <span class="material-symbols-outlined text-yt-primary/70 text-[18px]">folder_open</span>
                   <span class="text-xs text-yt-text truncate font-mono flex-1">{downloadPath}</span>
                </button>
             </div>
             
             <!-- Format & Quality -->
             <div class="flex items-center gap-2 shrink-0 bg-yt-bg border border-yt-border rounded px-2 py-1">
                <select bind:value={format} class="bg-transparent border-none p-0 text-xs text-yt-text font-medium focus:ring-0 cursor-pointer w-16">
                  <option value="mp4">MP4</option>
                  <option value="mp3">MP3</option>
                  <option value="mkv">MKV</option>
                </select>
                <div class="h-3 w-px bg-yt-border"></div>
                <select bind:value={quality} class="bg-transparent border-none p-0 text-xs text-yt-text font-medium focus:ring-0 cursor-pointer w-20">
                  <option value="best">Best</option>
                  <option value="1080p">1080p</option>
                  <option value="720p">720p</option>
                  <option value="480p">480p</option>
                </select>
             </div>
          </div>

          <!-- Bottom Row: Filename Options -->
          <div class="flex items-center gap-4 text-xs text-yt-text-secondary border-t border-yt-border/50 pt-2 px-1 cursor-default">
             <span class="font-medium text-yt-text-secondary w-auto shrink-0 opacity-70">{t("download.include")}</span>
             <label class="flex items-center gap-1.5 hover:text-yt-text transition-colors">
                <input type="checkbox" bind:checked={templateUploaderFolder} onchange={saveTemplateSettings} class="rounded border-yt-border text-yt-primary focus:ring-0 w-3.5 h-3.5 cursor-default" />
                <span>{t("download.uploaderFolder")}</span>
             </label>
             <label class="flex items-center gap-1.5 hover:text-yt-text transition-colors">
                <input type="checkbox" bind:checked={templateUploadDate} onchange={saveTemplateSettings} class="rounded border-yt-border text-yt-primary focus:ring-0 w-3.5 h-3.5 cursor-default" />
                <span>{t("download.uploadDate")}</span>
             </label>
             <label class="flex items-center gap-1.5 hover:text-yt-text transition-colors">
                <input type="checkbox" bind:checked={templateVideoId} onchange={saveTemplateSettings} class="rounded border-yt-border text-yt-primary focus:ring-0 w-3.5 h-3.5 cursor-default" />
                <span>{t("download.videoId")}</span>
             </label>
          </div>

          <!-- Cookie Browser & Concurrent Downloads -->
          <div class="flex items-center gap-4 text-xs text-yt-text-secondary border-t border-yt-border/50 pt-2 px-1 cursor-default">
             <!-- Cookie Browser -->
             <div class="flex items-center gap-1.5">
                <span class="material-symbols-outlined text-[16px] text-amber-500">cookie</span>
                <span
                  role="note"
                  class="font-medium text-yt-text-secondary shrink-0 opacity-70"
                  onmouseenter={(e) => showTooltip(e, t("settings.cookieHelp"))}
                  onmouseleave={hideTooltip}
                >{t("download.cookie")}</span>
                <select
                  class="bg-transparent border-none p-0 text-xs text-yt-text font-medium focus:ring-0 cursor-default"
                  bind:value={cookieBrowser}
                  onchange={() => autoSaveSettings({ cookieBrowser })}
                >
                  <option value={null}>{t("settings.none")}</option>
                  {#each browsers as browser}
                    <option value={browser}>{browser}</option>
                  {/each}
                </select>
             </div>

             {#if currentPlatform === "windows" && cookieBrowser && ["chrome", "edge", "brave"].some(b => cookieBrowser?.toLowerCase().includes(b))}
               <p class="text-[10px] text-amber-500">{t("settings.chromiumCookieWarning")}</p>
             {/if}

             <div class="h-3 w-px bg-yt-border/50"></div>

             <!-- Concurrent Downloads -->
             <div class="flex items-center gap-1.5 flex-1">
                <span class="material-symbols-outlined text-[16px] text-yt-primary">bolt</span>
                <span
                  role="note"
                  class="font-medium text-yt-text-secondary shrink-0 opacity-70"
                  onmouseenter={(e) => showTooltip(e, t("settings.concurrentDesc"))}
                  onmouseleave={hideTooltip}
                >{t("download.concurrent")}</span>
                <input
                  type="range"
                  class="flex-1 max-w-24 accent-yt-primary h-1"
                  min="1" max="10"
                  bind:value={maxConcurrent}
                  onchange={() => autoSaveSettings({ maxConcurrent })}
                />
                <span class="text-xs font-mono text-yt-text font-bold w-4 text-center">{maxConcurrent}</span>
             </div>
          </div>
       </div>
    </div>

    <!-- Results Area -->
    <div class="flex-1 overflow-y-auto px-6 pb-6">
       <div class="max-w-3xl mx-auto w-full">
         
         <!-- Analyzing Skeleton / Progress (only when no quickInfo yet) -->
    {#if analyzing && !quickInfo}
      <div class="flex-1 flex items-center justify-center p-8 animate-fade-in">
         <div class="max-w-md w-full bg-yt-surface border border-yt-border rounded-2xl p-6 shadow-xl relative overflow-hidden">
             <!-- Shimmer Overlay -->
             <div class="absolute inset-0 z-10 animate-shimmer pointer-events-none"></div>

             <div class="flex items-start gap-4 flex-col items-center text-center">
                <div class="w-16 h-16 rounded-xl bg-yt-highlight animate-pulse relative overflow-hidden flex items-center justify-center">
                   <span class="material-symbols-outlined text-yt-text-secondary/30 text-3xl">play_circle</span>
                </div>
                <div class="space-y-3 w-full flex flex-col items-center">
                   <div class="h-4 bg-yt-highlight rounded w-3/4 animate-pulse"></div>
                   <div class="h-3 bg-yt-highlight rounded w-1/2 animate-pulse"></div>
                </div>
                <div class="mt-4 flex items-center gap-2 text-xs text-yt-primary font-medium">
                   <span class="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>
                   <span>{t("download.analyzing")}</span>
                   {#if analyzeElapsed > 0}<span class="font-mono opacity-70">({analyzeElapsed}s)</span>{/if}
                </div>
             </div>
         </div>
      </div>
    {/if}

    <!-- Quick Preview Card (oEmbed result, while formats are loading) -->
    {#if quickInfo && !videoInfo}
      <div class="bg-yt-surface border border-yt-border rounded-xl overflow-hidden shadow-sm animate-scale-in">
         <div class="p-4 flex gap-4">
            <img src={quickInfo.thumbnail} alt="" class="w-48 h-28 rounded-lg object-cover shadow-sm bg-black/5" />
            <div class="flex-1 min-w-0 flex flex-col justify-between py-1">
               <div>
                  <h3 class="font-display font-semibold text-lg text-yt-text leading-tight mb-1">{quickInfo.title}</h3>
                  <p class="text-sm text-yt-text-secondary">{quickInfo.channel}</p>
               </div>
               <div class="flex items-center gap-2 text-xs text-yt-primary font-medium mt-2">
                  <span class="material-symbols-outlined text-[16px] animate-spin">progress_activity</span>
                  <span>{t("download.loadingFormats")}</span>
               </div>
            </div>
         </div>
      </div>
    {:else if videoInfo}
           <div class="bg-yt-surface border border-yt-border rounded-xl overflow-hidden shadow-sm animate-scale-in">
              <div class="p-4 flex gap-4">
                 {#if videoInfo.thumbnail}
                  <img src={videoInfo.thumbnail} alt="" class="w-48 h-28 rounded-lg object-cover shadow-sm bg-black/5" />
                {/if}
                <div class="flex-1 min-w-0 flex flex-col justify-between py-1">
                   <div>
                      <h3 class="font-display font-semibold text-lg text-yt-text leading-tight mb-1">{videoInfo.title}</h3>
                      <p class="text-sm text-yt-text-secondary">{videoInfo.channel}</p>
                   </div>
                   <div class="flex items-center gap-3 text-xs text-yt-text-muted mt-2">
                      <span class="flex items-center gap-1"><span class="material-symbols-outlined text-[14px]">schedule</span> {formatDuration(videoInfo.duration)}</span>
                      {#if playlistResult}
                         <button onclick={() => videoInfo = null} class="ml-auto text-yt-primary hover:underline">{t("download.backToPlaylist")}</button>
                      {/if}
                   </div>
                </div>
              </div>
           </div>
         {/if}

         <!-- Playlist Result -->
         {#if playlistResult}
           <div class="bg-yt-surface border border-yt-border rounded-xl overflow-hidden shadow-sm animate-scale-in">
              <!-- Header -->
              <div class="px-4 py-3 border-b border-yt-border flex items-center justify-between bg-yt-surface/50">
                 <div class="flex items-center gap-3 min-w-0">
                    <div class="w-8 h-8 rounded-lg bg-yt-primary/10 flex items-center justify-center text-yt-primary">
                       <span class="material-symbols-outlined text-[20px]">playlist_play</span>
                    </div>
                    <div class="min-w-0">
                       <h3 class="text-sm font-semibold text-yt-text truncate">{playlistResult.title}</h3>
                       <p class="text-xs text-yt-text-secondary">{playlistResult.videoCount ?? playlistResult.entries.length} videos</p>
                    </div>
                 </div>
                 
                 <div class="flex items-center gap-2">
                    <button
                      class="px-2 py-1 rounded hover:bg-yt-highlight text-xs font-medium text-yt-text-secondary transition-colors"
                      onclick={toggleSelectAll}
                    >
                      {allSelected ? t("download.deselect") : t("download.selectAll")}
                    </button>
                 </div>
              </div>

              <!-- List -->
              <div class="divide-y divide-yt-border/50">
                 {#each playlistResult.entries as entry, i}
                    <div class="group flex items-center gap-3 p-3 hover:bg-yt-highlight/50 transition-colors {selectedEntries.has(entry.videoId) ? 'bg-yt-primary/5' : ''}">
                       <!-- Checkbox -->
                       <div class="shrink-0 pl-1">
                          <input
                            type="checkbox"
                            checked={selectedEntries.has(entry.videoId)}
                            onchange={() => toggleSelect(entry.videoId)}
                            class="w-4 h-4 rounded border-yt-border text-yt-primary focus:ring-yt-primary cursor-pointer"
                          />
                       </div>
                       
                       <!-- Thumbnail -->
                       <button class="shrink-0 relative group/thumb cursor-pointer border-none bg-transparent p-0" onclick={() => handleSelectVideo(entry)}>
                          {#if entry.thumbnail}
                            <img src={entry.thumbnail} alt="" class="w-20 h-12 rounded-md object-cover bg-yt-highlight" />
                          {:else}
                            <div class="w-20 h-12 rounded-md bg-yt-highlight flex items-center justify-center">
                               <span class="material-symbols-outlined text-yt-text-muted">movie</span>
                            </div>
                          {/if}
                          <div class="absolute inset-0 bg-black/40 flex items-center justify-center opacity-0 group-hover/thumb:opacity-100 transition-opacity rounded-md">
                             <span class="material-symbols-outlined text-white text-[20px]">play_arrow</span>
                          </div>
                       </button>
                       
                       <!-- Info -->
                       <button class="flex-1 min-w-0 cursor-pointer text-left border-none bg-transparent p-0" onclick={() => handleSelectVideo(entry)}>
                          <p class="text-sm text-yt-text font-medium truncate">{entry.title || t("download.noTitle")}</p>
                          <p class="text-xs text-yt-text-secondary mt-0.5 flex items-center gap-2">
                             <span>{formatDuration(entry.duration)}</span>
                          </p>
                       </button>
                    </div>
                 {/each}
              </div>
              
              {#if !noMoreEntries && (playlistResult.videoCount == null || playlistResult.entries.length < playlistResult.videoCount)}
                 <div class="p-2 border-t border-yt-border">
                    <button 
                      class="w-full py-2 text-xs font-medium text-yt-text-secondary hover:text-yt-text hover:bg-yt-highlight rounded transition-colors"
                      onclick={handleLoadMore}
                      disabled={loadingMore}
                    >
                      {loadingMore ? t("download.loading") : t("download.loadMore")}
                    </button>
                 </div>
              {/if}
           </div>
         {/if}
         
         <!-- Empty State (No URL, No Info) -->
         {#if !videoInfo && !quickInfo && !playlistResult && !url}
            <div class="flex flex-col items-center justify-center py-20 opacity-50 select-none">
               <span class="material-symbols-outlined text-6xl text-yt-text-border mb-4">download_for_offline</span>
               <p class="text-yt-text-secondary text-sm">{t("download.emptyState")}</p>
            </div>
         {/if}
       </div>
    </div>
</div>

<style>
  @keyframes scale-in {
    from { opacity: 0; transform: scale(0.98); }
    to { opacity: 1; transform: scale(1); }
  }
  .animate-scale-in {
    animation: scale-in 0.2s ease-out;
  }
</style>
