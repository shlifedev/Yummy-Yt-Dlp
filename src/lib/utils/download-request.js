// @ts-check

/**
 * @typedef {{
 *   url: string;
 *   videoId: string;
 *   title: string;
 * }} SingleVideoInfo
 */

/**
 * @param {{
 *   videoInfo: SingleVideoInfo | null | undefined;
 *   url: string;
 *   formatId: string;
 *   qualityLabel: string;
 *   audioFormat: string | null;
 *   audioQuality: string | null;
 * }} options
 */
export function buildSingleDownloadRequest(options) {
  if (!options.videoInfo) return null

  return {
    videoUrl: options.videoInfo.url,
    videoId: options.videoInfo.videoId,
    title: options.videoInfo.title,
    formatId: options.formatId,
    qualityLabel: options.qualityLabel,
    outputDir: null,
    cookieBrowser: null,
    audioFormat: options.audioFormat,
    audioQuality: options.audioQuality,
  }
}
