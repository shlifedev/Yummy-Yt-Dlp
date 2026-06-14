import type { DownloadStatus, DownloadTaskInfo, HistoryEntry } from "$lib/bindings"

export type ActiveFilter = DownloadStatus | null

export type ActiveFilterOption = {
  key: ActiveFilter
  labelKey: string
  count: number
}

export type ActiveRow =
  | { kind: "group"; groupId: number; title: string; items: DownloadTaskInfo[] }
  | { kind: "single"; item: DownloadTaskInfo }

export function buildActiveRows(items: DownloadTaskInfo[]): ActiveRow[] {
  const rows: ActiveRow[] = []
  const groups = new Map<number, Extract<ActiveRow, { kind: "group" }>>()

  for (const item of items) {
    if (item.groupId != null) {
      let group = groups.get(item.groupId)
      if (!group) {
        group = {
          kind: "group",
          groupId: item.groupId,
          title: item.groupTitle || "-",
          items: [],
        }
        groups.set(item.groupId, group)
        rows.push(group)
      }
      group.items.push(item)
    } else {
      rows.push({ kind: "single", item })
    }
  }

  return rows
}

export function buildActiveFilters(active: DownloadTaskInfo[], inProgress: DownloadTaskInfo[]): ActiveFilterOption[] {
  return [
    { key: null, labelKey: "queue.all", count: inProgress.length },
    { key: "downloading", labelKey: "queue.downloading", count: active.filter((i) => i.status === "downloading").length },
    { key: "pending", labelKey: "queue.pending", count: active.filter((i) => i.status === "pending").length },
    { key: "failed", labelKey: "queue.failed", count: active.filter((i) => i.status === "failed").length },
    { key: "cancelled", labelKey: "queue.cancelled", count: active.filter((i) => i.status === "cancelled").length },
    { key: "completed", labelKey: "queue.completed", count: active.filter((i) => i.status === "completed").length },
  ]
}

export function nextGroupMaxCounts(current: Map<number, number>, items: DownloadTaskInfo[]): Map<number, number> {
  const counts = new Map<number, number>()
  for (const item of items) {
    if (item.groupId != null) {
      counts.set(item.groupId, (counts.get(item.groupId) ?? 0) + 1)
    }
  }

  let changed = false
  const next = new Map(current)
  for (const [groupId, count] of counts) {
    if (count > (next.get(groupId) ?? 0)) {
      next.set(groupId, count)
      changed = true
    }
  }

  return changed ? next : current
}

export function groupDenominator(groupMaxCount: Map<number, number>, groupId: number, visible: number): number {
  return Math.max(visible, groupMaxCount.get(groupId) ?? 0)
}

export function groupDone(items: DownloadTaskInfo[]): number {
  return items.filter((item) => item.status === "completed").length
}

export function groupProgress(groupMaxCount: Map<number, number>, groupId: number, items: DownloadTaskInfo[]): number {
  const denominator = groupDenominator(groupMaxCount, groupId, items.length)
  if (!denominator) return 0

  const sum = items.reduce((total, item) => {
    return total + (item.status === "completed" ? 100 : item.progress || 0)
  }, 0)

  return Math.round(sum / denominator)
}

export function historyEntryKey(entry: HistoryEntry): string {
  return entry.kind === "group" ? `g${entry.group.groupId}` : `s${entry.item.id}`
}

export function activeRowKey(row: ActiveRow): string {
  return row.kind === "group" ? `g${row.groupId}` : `s${row.item.id}`
}
