import type { AdvancedOptions, DownloadRequest, PlaylistEntry } from "$lib/bindings"

export const downloadFormats = ["mp4", "mkv", "mp3", "flac", "opus", "wav"] as const
export type DownloadFormat = (typeof downloadFormats)[number]

export type PlaylistViewMeta = {
  playlistId: string
  title: string
  url: string
  channelName: string | null
  videoCount: number | null
}

export type PlaylistView = PlaylistViewMeta & { entries: PlaylistEntry[] }
export type PlaylistDownloadEntry = Pick<PlaylistEntry, "url" | "videoId" | "title">

export type BatchProgress = { current: number; total: number }
export type SkippedDetails = { queue: string[]; exists: string[] }

export function isAudioDownloadFormat(format: DownloadFormat): boolean {
  return format === "mp3" || format === "flac" || format === "opus" || format === "wav"
}

export function isLosslessDownloadFormat(format: DownloadFormat): boolean {
  return format === "flac" || format === "wav"
}

export function countActiveAdvancedOptions(advanced: AdvancedOptions): number {
  return [
    advanced.writeSubs,
    advanced.writeAutoSubs,
    advanced.embedSubs,
    advanced.subLangs !== "en",
    advanced.convertSubs !== "",
    advanced.sponsorblockMode !== "off",
    advanced.embedThumbnail,
    advanced.embedMetadata,
    advanced.embedChapters,
    advanced.writeThumbnail,
    advanced.writeInfoJson,
    advanced.videoCodec !== "auto",
    advanced.limitRate !== "",
    advanced.concurrentFragments !== 1,
    advanced.retries != null,
    advanced.sleepInterval !== 0,
    advanced.mergeOutputFormat !== "",
    advanced.remuxVideo !== "",
    advanced.downloadSections !== "",
    advanced.splitChapters,
    advanced.proxy !== "",
    advanced.noMtime,
    advanced.restrictFilenames,
  ].filter(Boolean).length
}

export function buildTemplate(options: {
  templateUploaderFolder: boolean
  templateUploadDate: boolean
  templateVideoId: boolean
}): string {
  let name = "%(title)s"
  if (options.templateUploadDate) name = "%(upload_date)s " + name
  if (options.templateVideoId) name = name + " [%(id)s]"
  let path = name + ".%(ext)s"
  if (options.templateUploaderFolder) path = "%(uploader)s/" + path
  return path
}

export function getTemplatePreview(options: {
  useAdvancedTemplate: boolean
  filenameTemplate: string
  templateUploaderFolder: boolean
  templateUploadDate: boolean
  templateVideoId: boolean
}): string {
  if (options.useAdvancedTemplate) return options.filenameTemplate
  let name = "Title"
  if (options.templateUploadDate) name = "20240101 " + name
  if (options.templateVideoId) name = name + " [dQw4w9WgXcQ]"
  let path = name + ".mp4"
  if (options.templateUploaderFolder) path = "Uploader/" + path
  return path
}

export function buildFormatString(format: DownloadFormat, quality: string): string {
  if (isAudioDownloadFormat(format)) return "bestaudio/best"

  let height = ""
  if (quality === "1080p") height = "[height<=1080]"
  else if (quality === "720p") height = "[height<=720]"
  else if (quality === "480p") height = "[height<=480]"

  if (format === "mp4") {
    return `bestvideo${height}[ext=mp4]+bestaudio[ext=m4a]/best${height}[ext=mp4]/best${height}`
  }
  return `bestvideo${height}+bestaudio/best${height}`
}

export function buildQualityLabel(format: DownloadFormat, quality: string, audioQuality: string): string {
  if (!isAudioDownloadFormat(format)) return quality === "best" ? "Best" : quality
  return isLosslessDownloadFormat(format) ? "Lossless" : audioQuality === "0" ? "Best" : audioQuality
}

export function buildDownloadRequests(
  entries: PlaylistDownloadEntry[],
  options: {
    formatId: string
    qualityLabel: string
    format: DownloadFormat
    audioQuality: string
  },
): DownloadRequest[] {
  const isAudio = isAudioDownloadFormat(options.format)
  const isLossless = isLosslessDownloadFormat(options.format)

  return entries.map((entry) => ({
    videoUrl: entry.url,
    videoId: entry.videoId,
    title: entry.title || `Video ${entry.videoId}`,
    formatId: options.formatId,
    qualityLabel: options.qualityLabel,
    outputDir: null,
    cookieBrowser: null,
    audioFormat: isAudio ? options.format : null,
    audioQuality: isAudio && !isLossless ? options.audioQuality : null,
  }))
}
