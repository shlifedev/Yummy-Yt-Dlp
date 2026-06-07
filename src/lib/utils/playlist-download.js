// @ts-check

/**
 * @typedef {{ url: string; videoCount?: number | null; entries: unknown[] }} DownloadablePlaylist
 */

/**
 * @param {DownloadablePlaylist | null} playlist
 * @param {boolean} noMoreEntries
 */
export function isPlaylistCompleteForDownload(playlist, noMoreEntries) {
  if (!playlist) return false
  if (noMoreEntries) return true
  return playlist.videoCount != null && playlist.entries.length >= playlist.videoCount
}

/**
 * Waits for any in-flight playlist/channel auto-load before returning the entries that should be
 * queued. If the auto-loader is not running and the current list is still incomplete, it performs
 * one full fetch as a fallback.
 *
 * @template {DownloadablePlaylist} T
 * @param {{
 *   getPlaylist: () => T | null;
 *   getNoMoreEntries: () => boolean;
 *   getAutoLoadPromise: () => Promise<void> | null;
 *   fetchFullPlaylist: (url: string) => Promise<T>;
 *   setPlaylist: (playlist: T) => void;
 *   markNoMoreEntries: () => void;
 *   isCurrent?: () => boolean;
 * }} options
 * @returns {Promise<T | null>}
 */
export async function getPlaylistReadyForDownload(options) {
  const isCurrent = options.isCurrent ?? (() => true)
  const pendingAutoLoad = options.getAutoLoadPromise()

  if (pendingAutoLoad) {
    await pendingAutoLoad
    if (!isCurrent()) return null
  }

  const playlist = options.getPlaylist()
  if (!playlist) return null

  if (isPlaylistCompleteForDownload(playlist, options.getNoMoreEntries())) {
    return playlist
  }

  const fullPlaylist = await options.fetchFullPlaylist(playlist.url)
  if (!isCurrent()) return null

  options.setPlaylist(fullPlaylist)
  options.markNoMoreEntries()
  return fullPlaylist
}
