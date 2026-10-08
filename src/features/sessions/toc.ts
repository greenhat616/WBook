import type { TextRange, TocEntry, TocEntry_Serialize } from '@/bindings'

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

export function renameTocEntry(
  entries: TocEntry_Serialize[],
  id: number,
  title: string
): TocEntry_Serialize[] {
  return entries.map((entry) =>
    entry.id === id
      ? { ...entry, title }
      : { ...entry, children: renameTocEntry(entry.children, id, title) }
  )
}

/**
 * Removes one entry and lifts its children into its place. Heading text stays
 * in the book, because a chapter runs until the next heading. Body ranges
 * must tile the document, so a removed body joins its neighbor instead.
 */
export function removeTocEntry(
  entries: TocEntry_Serialize[],
  id: number
): TocEntry_Serialize[] {
  const removed = findEntry(entries, id)
  if (!removed) return entries
  const range = removed.meta.range
  let merge: { id: number; range: TextRange } | null = null
  if (removed.meta.range_kind === 'Body' && range) {
    const bodies = bodyEntries(entries).filter((entry) => entry.id !== id)
    const before = bodies.filter((entry) => entry.range.end <= range.start)
    const previous = before[before.length - 1]
    const next = bodies.find((entry) => entry.range.start >= range.end)
    if (previous) {
      merge = {
        id: previous.id,
        range: { start: previous.range.start, end: range.end }
      }
    } else if (next) {
      merge = {
        id: next.id,
        range: { start: range.start, end: next.range.end }
      }
    }
  }
  const prune = (list: TocEntry_Serialize[]): TocEntry_Serialize[] =>
    list.flatMap((entry) => {
      if (entry.id === id) return prune(entry.children)
      const meta =
        entry.id === merge?.id
          ? { ...entry.meta, range: merge.range }
          : entry.meta
      return [{ ...entry, meta, children: prune(entry.children) }]
    })
  return prune(entries)
}

function findEntry(
  entries: TocEntry_Serialize[],
  id: number
): TocEntry_Serialize | null {
  for (const entry of entries) {
    if (entry.id === id) return entry
    const found = findEntry(entry.children, id)
    if (found) return found
  }
  return null
}

function bodyEntries(
  entries: TocEntry_Serialize[]
): Array<{ id: number; range: TextRange }> {
  const bodies: Array<{ id: number; range: TextRange }> = []
  const walk = (list: TocEntry_Serialize[]) => {
    for (const entry of list) {
      if (entry.meta.range_kind === 'Body' && entry.meta.range)
        bodies.push({ id: entry.id, range: entry.meta.range })
      walk(entry.children)
    }
  }
  walk(entries)
  return bodies.sort((a, b) => a.range.start - b.range.start)
}
