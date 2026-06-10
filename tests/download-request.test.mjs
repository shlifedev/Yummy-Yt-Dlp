import assert from "node:assert/strict"
import { test } from "node:test"

import { buildSingleDownloadRequest } from "../src/lib/utils/download-request.js"

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
