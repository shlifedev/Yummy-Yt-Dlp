import assert from "node:assert/strict"
import { test } from "node:test"

import { buildSingleDownloadRequest } from "../src/lib/utils/download-request.js"
import { getPlaylistReadyForDownload } from "../src/lib/utils/playlist-download.js"

test("does not build a single download request from a raw URL before metadata is ready", () => {
  const request = buildSingleDownloadRequest({
    videoInfo: null,
    url: "https://www.youtube.com/@example",
    formatId: "bestvideo+bestaudio/best",
    qualityLabel: "Best",
    audioFormat: null,
    audioQuality: null,
  })

  assert.equal(request, null)
})

test("waits for playlist auto-loading before returning entries for download", async () => {
  let playlist = {
    title: "Channel uploads",
    url: "https://www.youtube.com/@example",
    videoCount: 2,
    entries: [{ videoId: "a", url: "https://www.youtube.com/watch?v=a" }],
  }
  let noMoreEntries = false
  let fullFetchCount = 0

  const autoLoadPromise = Promise.resolve().then(() => {
    playlist = {
      ...playlist,
      entries: [
        ...playlist.entries,
        { videoId: "b", url: "https://www.youtube.com/watch?v=b" },
      ],
    }
    noMoreEntries = true
  })

  const ready = await getPlaylistReadyForDownload({
    getPlaylist: () => playlist,
    getNoMoreEntries: () => noMoreEntries,
    getAutoLoadPromise: () => autoLoadPromise,
    fetchFullPlaylist: async () => {
      fullFetchCount += 1
      return playlist
    },
    setPlaylist: (next) => {
      playlist = next
    },
    markNoMoreEntries: () => {
      noMoreEntries = true
    },
  })

  assert.equal(fullFetchCount, 0)
  assert.equal(ready.entries.length, 2)
})
