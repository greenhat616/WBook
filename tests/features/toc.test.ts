import { describe, expect, it } from 'vitest'
import type { TocEntry_Serialize, TocRangeKind } from '@/bindings'
import { removeTocEntry, renameTocEntry } from '@/features/sessions/toc'

const entry = (
  id: number,
  start: number,
  end: number,
  kind: TocRangeKind = 'Heading',
  children: TocEntry_Serialize[] = []
): TocEntry_Serialize => ({
  id,
  title: `节点${id}`,
  meta: { words: 0, range_kind: kind, range: { start, end } },
  children
})

describe('TOC edits', () => {
  it('renames a nested entry without touching the others', () => {
    const toc = [entry(1, 0, 3, 'Heading', [entry(2, 4, 7)]), entry(3, 9, 12)]
    const renamed = renameTocEntry(toc, 2, '新名')
    expect(renamed[0].children[0].title).toBe('新名')
    expect(renamed[0].title).toBe('节点1')
    expect(toc[0].children[0].title).toBe('节点2')
  })

  it('lifts the children of a removed entry into its place', () => {
    const toc = [
      entry(1, 0, 3, 'Heading', [entry(2, 4, 7), entry(3, 9, 12)]),
      entry(4, 14, 17)
    ]
    expect(removeTocEntry(toc, 1).map((node) => node.id)).toEqual([2, 3, 4])
  })

  it('merges a removed body range into its neighbor', () => {
    const toc = [entry(1, 0, 10, 'Body'), entry(2, 10, 20, 'Body')]
    // Removing the first part gives its text to the next one.
    expect(removeTocEntry(toc, 1)).toEqual([
      { ...toc[1], meta: { ...toc[1].meta, range: { start: 0, end: 20 } } }
    ])
    expect(removeTocEntry(toc, 2)).toEqual([
      { ...toc[0], meta: { ...toc[0].meta, range: { start: 0, end: 20 } } }
    ])
  })
})
