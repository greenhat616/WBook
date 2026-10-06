import { useMemo, useState } from 'react'
import ExpandIcon from '~icons/material-symbols/chevron-right-rounded'
import { Button } from '@/components/ui/button'
import { cn } from '@/utils/ui'
import {
  flattenToc,
  formatBytes,
  summarizeToc,
  type TocRow,
  type TocSummary
} from '../toc'
import type { TocEntry } from '@/bindings'

type Props = {
  toc: TocEntry[] | null
  draft: TocEntry[] | null
  selected: number | null
  onSelect: (row: TocRow) => void
  onApplyDraft: () => void
  onDiscardDraft: () => void
  disabled: boolean
  emptyHint: string
}

function describe({ volumes, chapters }: TocSummary) {
  return volumes ? `${volumes} 卷 · ${chapters} 章` : `${chapters} 章`
}

export function TocPanel({
  toc,
  draft,
  selected,
  onSelect,
  onApplyDraft,
  onDiscardDraft,
  disabled,
  emptyHint
}: Props) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(new Set())
  const current = useMemo(() => (toc ? flattenToc(toc) : []), [toc])
  const proposed = useMemo(() => (draft ? flattenToc(draft) : null), [draft])
  const rows = proposed ?? current
  // Flat chapter lists need no gutter for expand buttons.
  const nested = rows.some((row) => row.hasChildren)

  // Hide every descendant of a collapsed row; rows arrive in document order.
  const visible = useMemo(() => {
    const hidden = new Set<number>()
    return rows.filter((row) => {
      if (row.parent !== null && hidden.has(row.parent)) {
        hidden.add(row.id)
        return false
      }
      if (collapsed.has(row.id)) hidden.add(row.id)
      return true
    })
  }, [rows, collapsed])

  function toggle(id: number) {
    setCollapsed((previous) => {
      const next = new Set(previous)
      if (!next.delete(id)) next.add(id)
      return next
    })
  }

  return (
    <section
      aria-labelledby="toc-heading"
      className="flex min-h-0 flex-col overflow-hidden rounded-2xl bg-card"
    >
      <header className="flex h-10 shrink-0 items-center justify-between gap-2 px-3">
        <h2 id="toc-heading" className="text-sm font-semibold">
          {proposed ? '试解析目录' : '目录'}
        </h2>
        {rows.length > 0 && (
          <span className="text-xs tabular-nums text-muted-foreground">
            {describe(summarizeToc(rows))}
          </span>
        )}
      </header>

      {proposed && (
        <div
          role="status"
          className="mx-2 mb-2 shrink-0 space-y-2 rounded-xl bg-tertiary p-2.5 text-xs text-tertiary-foreground"
        >
          <p>
            新目录 {describe(summarizeToc(proposed))}
            {toc && <>，当前 {describe(summarizeToc(current))}</>}
          </p>
          <div className="flex gap-1.5">
            <Button size="xs" onClick={onApplyDraft} disabled={disabled}>
              应用新目录
            </Button>
            <Button
              size="xs"
              variant="ghost"
              onClick={onDiscardDraft}
              className="text-tertiary-foreground"
            >
              放弃
            </Button>
          </div>
        </div>
      )}

      {rows.length === 0 ? (
        <p className="px-3 py-6 text-center text-xs text-muted-foreground">
          {emptyHint}
        </p>
      ) : (
        <ol className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-2 text-[13px]">
          {visible.map((row) => (
            <li
              key={row.id}
              className="flex items-center"
              style={{ paddingInlineStart: `${row.depth * 0.875}rem` }}
            >
              {row.hasChildren ? (
                <button
                  type="button"
                  aria-label={`${collapsed.has(row.id) ? '展开' : '收起'}${row.title}`}
                  aria-expanded={!collapsed.has(row.id)}
                  onClick={() => toggle(row.id)}
                  className="flex size-6 shrink-0 items-center justify-center rounded-full text-muted-foreground hover:bg-muted"
                >
                  <ExpandIcon
                    aria-hidden="true"
                    className={cn(
                      'size-4 transition-transform',
                      !collapsed.has(row.id) && 'rotate-90'
                    )}
                  />
                </button>
              ) : (
                nested && <span className="w-6 shrink-0" />
              )}
              <button
                type="button"
                aria-current={row.id === selected ? 'true' : undefined}
                onClick={() => onSelect(row)}
                className={cn(
                  'flex h-7 min-w-0 flex-1 items-center gap-2 rounded-full px-2 text-left hover:bg-muted',
                  row.hasChildren && 'font-semibold',
                  row.id === selected &&
                    'bg-secondary text-secondary-foreground hover:bg-secondary'
                )}
              >
                <span className="min-w-0 flex-1 truncate">
                  {row.title || '未命名章节'}
                </span>
                <span className="shrink-0 text-[11px] tabular-nums text-muted-foreground">
                  {row.span && !row.hasChildren
                    ? formatBytes(row.span.end - row.span.start)
                    : ''}
                </span>
              </button>
            </li>
          ))}
        </ol>
      )}
    </section>
  )
}
