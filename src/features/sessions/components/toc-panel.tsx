import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent
} from 'react'
import { createPortal } from 'react-dom'
import ExpandIcon from '~icons/material-symbols/chevron-right-rounded'
import DeleteIcon from '~icons/material-symbols/delete-outline-rounded'
import RenameIcon from '~icons/material-symbols/edit-outline-rounded'
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
  length: number | null
  selected: number | null
  onSelect: (row: TocRow) => void
  onApplyDraft: () => void
  onDiscardDraft: () => void
  onRename: (row: TocRow, title: string) => void
  onRemove: (row: TocRow) => void
  /** Edits need a TOC that matches the current text. */
  editable: boolean
  disabled: boolean
  emptyHint: string
}

function describe({ volumes, chapters }: TocSummary) {
  return volumes ? `${volumes} 卷 · ${chapters} 章` : `${chapters} 章`
}

export function TocPanel({
  toc,
  draft,
  length,
  selected,
  onSelect,
  onApplyDraft,
  onDiscardDraft,
  onRename,
  onRemove,
  editable,
  disabled,
  emptyHint
}: Props) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<number>>(new Set())
  const [renaming, setRenaming] = useState<{
    id: number
    title: string
  } | null>(null)
  const [menu, setMenu] = useState<{
    row: TocRow
    x: number
    y: number
  } | null>(null)
  const canEdit = editable && !disabled
  const current = useMemo(
    () => (toc ? flattenToc(toc, length) : []),
    [toc, length]
  )
  const proposed = useMemo(
    () => (draft ? flattenToc(draft, length) : null),
    [draft, length]
  )
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

  function finishRename(row: TocRow) {
    if (renaming?.id !== row.id) return
    const title = renaming.title.trim()
    setRenaming(null)
    if (title && title !== row.title) onRename(row, title)
  }

  // Edit actions stay out of the row, where they would squeeze the titles;
  // the context menu and shortcuts reach them instead.
  function rowKeyDown(row: TocRow, event: KeyboardEvent<HTMLButtonElement>) {
    if (!canEdit) return
    if (event.key === 'F2') {
      event.preventDefault()
      setRenaming({ id: row.id, title: row.title })
    } else if (event.key === 'Delete') {
      event.preventDefault()
      onRemove(row)
    } else if (
      event.key === 'ContextMenu' ||
      (event.key === 'F10' && event.shiftKey)
    ) {
      event.preventDefault()
      const box = event.currentTarget.getBoundingClientRect()
      setMenu({ row, x: box.left + 16, y: box.bottom })
    }
  }

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
          {proposed ? '待应用的目录' : '目录'}
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
              {renaming?.id === row.id ? (
                <input
                  aria-label="章节名称"
                  autoFocus
                  value={renaming.title}
                  onChange={(event) =>
                    setRenaming({ id: row.id, title: event.target.value })
                  }
                  onBlur={() => finishRename(row)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter') finishRename(row)
                    else if (event.key === 'Escape') setRenaming(null)
                  }}
                  className="h-7 min-w-0 flex-1 rounded-full border border-ring bg-background px-2 outline-none"
                />
              ) : (
                <button
                  type="button"
                  aria-current={row.id === selected ? 'true' : undefined}
                  onClick={() => onSelect(row)}
                  onKeyDown={(event) => rowKeyDown(row, event)}
                  onContextMenu={(event) => {
                    if (!canEdit) return
                    event.preventDefault()
                    setMenu({ row, x: event.clientX, y: event.clientY })
                  }}
                  aria-keyshortcuts={canEdit ? 'F2 Delete' : undefined}
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
              )}
            </li>
          ))}
        </ol>
      )}
      {menu && (
        <RowMenu
          x={menu.x}
          y={menu.y}
          label={menu.row.title}
          onClose={() => setMenu(null)}
          onRename={() =>
            setRenaming({ id: menu.row.id, title: menu.row.title })
          }
          onRemove={() => onRemove(menu.row)}
        />
      )}
    </section>
  )
}

type RowMenuProps = {
  x: number
  y: number
  label: string
  onClose: () => void
  onRename: () => void
  onRemove: () => void
}

function RowMenu({ x, y, label, onClose, onRename, onRemove }: RowMenuProps) {
  const ref = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ x, y })

  // Keep the menu inside the window when opened near an edge.
  useLayoutEffect(() => {
    const box = ref.current!.getBoundingClientRect()
    setPosition({
      x: Math.max(0, Math.min(x, window.innerWidth - box.width - 4)),
      y: Math.max(0, Math.min(y, window.innerHeight - box.height - 4))
    })
    ref.current!.querySelector<HTMLElement>('[role="menuitem"]')?.focus()
  }, [x, y])

  useEffect(() => {
    const outside = (event: Event) => {
      if (!ref.current?.contains(event.target as Node)) onClose()
    }
    window.addEventListener('pointerdown', outside, true)
    window.addEventListener('scroll', onClose, true)
    window.addEventListener('resize', onClose)
    window.addEventListener('blur', onClose)
    return () => {
      window.removeEventListener('pointerdown', outside, true)
      window.removeEventListener('scroll', onClose, true)
      window.removeEventListener('resize', onClose)
      window.removeEventListener('blur', onClose)
    }
  }, [onClose])

  function choose(action: () => void) {
    onClose()
    action()
  }

  function keyDown(event: KeyboardEvent<HTMLDivElement>) {
    const items = [
      ...ref.current!.querySelectorAll<HTMLElement>('[role="menuitem"]')
    ]
    const index = items.indexOf(document.activeElement as HTMLElement)
    if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault()
      onClose()
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      const step = event.key === 'ArrowDown' ? 1 : -1
      items[(index + step + items.length) % items.length]?.focus()
    }
  }

  return createPortal(
    <div
      ref={ref}
      role="menu"
      aria-label={`${label}的操作`}
      onKeyDown={keyDown}
      onContextMenu={(event) => event.preventDefault()}
      style={{ left: position.x, top: position.y }}
      className="fixed z-50 min-w-36 rounded-xl bg-popover py-1 text-sm text-popover-foreground shadow-lg ring-1 ring-border"
    >
      <button
        type="button"
        role="menuitem"
        onClick={() => choose(onRename)}
        className="flex h-9 w-full items-center gap-3 px-3 text-left outline-none hover:bg-muted focus-visible:bg-muted"
      >
        <RenameIcon aria-hidden="true" className="size-4" />
        重命名
        <kbd className="ml-auto pl-4 text-xs text-muted-foreground">F2</kbd>
      </button>
      <button
        type="button"
        role="menuitem"
        onClick={() => choose(onRemove)}
        className="flex h-9 w-full items-center gap-3 px-3 text-left text-destructive outline-none hover:bg-muted focus-visible:bg-muted"
      >
        <DeleteIcon aria-hidden="true" className="size-4" />
        删除
        <kbd className="ml-auto pl-4 text-xs text-muted-foreground">Del</kbd>
      </button>
    </div>,
    document.body
  )
}
