import type { TextRange, TocEntry } from '@/bindings'

export type TocRow = {
  id: number
  title: string
  depth: number
  parent: number | null
  hasChildren: boolean
  /** Text from this heading up to the next one; null when the end is unknown. */
  span: TextRange | null
}

export type TocSummary = { volumes: number; chapters: number }

/**
 * Flattens the tree in document order. Heading ranges cover only the heading
 * line, so like the export planner a chapter runs until the next range starts;
 * the last one runs to the end of the document when its length is known.
 */
export function flattenToc(
  entries: TocEntry[],
  length: number | null = null
): TocRow[] {
  const rows: TocRow[] = []
  const ranges: Array<{ range: TextRange | null; body: boolean }> = []
  const walk = (list: TocEntry[], depth: number, parent: number | null) => {
    for (const entry of list) {
      rows.push({
        id: entry.id,
        title: entry.title,
        depth,
        parent,
        hasChildren: entry.children.length > 0,
        span: null
      })
      ranges.push({
        range: entry.meta.range,
        body: entry.meta.range_kind === 'Body'
      })
      walk(entry.children, depth + 1, entry.id)
    }
  }
  walk(entries, 0, null)

  let next = length
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const { range, body } = ranges[index]
    if (!range) continue
    if (body) rows[index].span = range
    else if (next !== null && next > range.start) {
      rows[index].span = { start: range.start, end: next }
    }
    next = range.start
  }
  return rows
}

export function summarizeToc(rows: TocRow[]): TocSummary {
  let volumes = 0
  let chapters = 0
  for (const row of rows) {
    if (row.hasChildren) volumes += 1
    else chapters += 1
  }
  return { volumes, chapters }
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} K`
  return `${(bytes / 1024 / 1024).toFixed(1)} M`
}
