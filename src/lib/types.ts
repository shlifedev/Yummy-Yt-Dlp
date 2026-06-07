export interface ActiveDownload {
  id: number
  title: string
  videoUrl: string
  videoId: string
  formatId: string
  qualityLabel: string
  outputPath: string
  status: string
  progress: number
  speed: string | null
  eta: string | null
  errorMessage: string | null
  createdAt: number
  completedAt: number | null
  groupTitle?: string | null
}

export interface ProgressCacheEntry {
  progress: number
  speed: string | null
  eta: string | null
}
