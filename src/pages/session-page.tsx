import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type FormEvent,
  type ReactNode
} from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { Link, useNavigate } from '@tanstack/react-router'
import ArrowLeftIcon from '~icons/material-symbols/arrow-back-rounded'
import CheckIcon from '~icons/material-symbols/check-rounded'
import CloseIcon from '~icons/material-symbols/close-rounded'
import DownloadIcon from '~icons/material-symbols/download-rounded'
import EyeIcon from '~icons/material-symbols/visibility-outline-rounded'
import FileTextIcon from '~icons/material-symbols/description-outline-rounded'
import FolderIcon from '~icons/material-symbols/folder-open-outline-rounded'
import RefreshIcon from '~icons/material-symbols/refresh-rounded'
import SettingsIcon from '~icons/material-symbols/settings-outline-rounded'
import StopIcon from '~icons/material-symbols/stop-rounded'
import WarningIcon from '~icons/material-symbols/warning-outline-rounded'
import {
  commands,
  type Activity,
  type Metadata,
  type OpKind,
  type Phase,
  type WorkspaceStatus
} from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { errorMessage } from '@/features/sessions/api'
import { BookInfoPanel } from '@/features/sessions/components/book-info-panel'
import { ParserPanel } from '@/features/sessions/components/parser-panel'
import { PreviewPanel } from '@/features/sessions/components/preview-panel'
import { TextPanel } from '@/features/sessions/components/text-panel'
import { TocPanel } from '@/features/sessions/components/toc-panel'
import {
  flattenToc,
  removeTocEntry,
  renameTocEntry
} from '@/features/sessions/toc'
import { useSession } from '@/features/sessions/use-session'
import { cn } from '@/utils/ui'

const operationLabels: Record<OpKind, string> = {
  Initialize: '整理文本',
  Parse: '解析目录',
  Install: '应用目录',
  Edit: '更新文本',
  SetMetadataOverrides: '更新书籍信息',
  SetSettings: '更新本书设置',
  SetCoverImage: '更换封面',
  RenderCover: '生成封面',
  ReadText: '读取文本',
  ReadResults: '读取整理结果',
  RenderPreview: '生成预览',
  ExportEpub: '导出 EPUB'
}

function phaseLabel(phase: Phase | null): string | null {
  if (phase === null) return null
  if (typeof phase === 'object') {
    return `清理内容 ${phase.Filtering.index + 1}/${phase.Filtering.total}`
  }
  const labels: Record<Exclude<Phase, object>, string> = {
    Extracting: '读取原始文本',
    Parsing: '识别目录与信息',
    Installing: '应用整理结果',
    Editing: '更新内容',
    Reading: '读取内容',
    Rendering: '排版预览',
    Exporting: '打包 EPUB'
  }
  return labels[phase]
}

type Tab = 'text' | 'preview' | 'book' | 'parser'
const tabs: Array<{ value: Tab; label: string }> = [
  { value: 'text', label: '正文' },
  { value: 'preview', label: '预览' },
  { value: 'book', label: '书籍信息' },
  { value: 'parser', label: '解析规则' }
]

export function SessionPage({ sessionId }: { sessionId: number }) {
  const session = useSession(sessionId)
  const {
    snapshot,
    results,
    preview,
    draft,
    error,
    warnings,
    loading,
    pending,
    connection,
    notice,
    exportPath
  } = session
  const navigate = useNavigate()
  const windowed = isTauri()
  const [tab, setTab] = useState<Tab>('text')
  // Node IDs are reused across parses, so a selection also matches its title.
  const [selection, setSelection] = useState<{
    id: number
    title: string
  } | null>(null)
  const [destination, setDestination] = useState('')
  const [closing, setClosing] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [localError, setLocalError] = useState<string | null>(null)
  const shownToc = draft?.toc ?? results?.results?.toc ?? null
  const length = snapshot?.workspace_status.Available?.document_len ?? null
  const rows = useMemo(
    () => (shownToc ? flattenToc(shownToc, length) : []),
    [shownToc, length]
  )
  const selected =
    rows.find(
      (row) => row.id === selection?.id && row.title === selection.title
    ) ?? null
  const name = snapshot
    ? snapshot.source.split(/[\\/]/).pop() || snapshot.source
    : ''

  // Opening a book should show its chapters without an extra click. Each
  // session is tried once; after a failure the button retries, so an error
  // is never repeated in a loop.
  const attempted = useRef<number | null>(null)
  const unparsed =
    snapshot?.lifecycle === 'Open' &&
    connection !== 'closed' &&
    ['Absent', 'Unparsed'].includes(
      snapshot.workspace_status.Available?.document ?? ''
    )
  const idle = snapshot?.activity === 'Idle' && !pending && !loading
  const { initialize } = session
  useEffect(() => {
    if (!unparsed || !idle || attempted.current === sessionId) return
    attempted.current = sessionId
    void initialize()
  }, [unparsed, idle, sessionId, initialize])

  async function closeSession() {
    setClosing(true)
    const report = await session.close()
    setClosing(false)
    if (report && report.cleanup_failures.length === 0)
      await navigate({ to: '/' })
  }

  async function cancelOperation() {
    setCancelling(true)
    await session.cancel()
    setCancelling(false)
  }

  async function openSettings() {
    setLocalError(null)
    try {
      await commands.openSessionSettingsWindow(sessionId)
    } catch (cause) {
      setLocalError(errorMessage(cause))
    }
  }

  async function chooseDestination(): Promise<string | null> {
    setLocalError(null)
    try {
      const path = await saveDialog({
        defaultPath: `${(results?.overrides.title ?? results?.results?.metadata.title) || name}.epub`,
        filters: [{ name: 'EPUB', extensions: ['epub'] }]
      })
      if (path) setDestination(path)
      return path
    } catch (cause) {
      setLocalError(errorMessage(cause))
      return null
    }
  }

  async function exportBook(event?: FormEvent<HTMLFormElement>) {
    event?.preventDefault()
    const target =
      destination.trim() || (windowed ? await chooseDestination() : null)
    if (target) void session.exportBook(target)
  }

  const backLink = !windowed && (
    <Button asChild variant="ghost" size="icon-sm" className="shrink-0">
      <Link to="/" aria-label="返回工作台" title="返回工作台">
        <ArrowLeftIcon aria-hidden="true" />
      </Link>
    </Button>
  )

  if (!snapshot) {
    return (
      <div className="flex flex-1 flex-col gap-3 p-3">
        <div className="flex items-center gap-2">
          {backLink}
          <h1 className="text-base font-semibold">
            {loading ? '正在打开工作会话…' : '暂时无法打开这份内容'}
          </h1>
        </div>
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        {!loading && (
          <Button
            variant="outline"
            size="sm"
            className="self-start"
            onClick={() => void session.refresh()}
          >
            <RefreshIcon aria-hidden="true" />
            重新尝试
          </Button>
        )}
      </div>
    )
  }

  const status = snapshot.workspace_status.Available
  const closed = connection === 'closed' || snapshot.lifecycle === 'Closed'
  const open = snapshot.lifecycle === 'Open' && !closed
  const running =
    !open || snapshot.activity === 'Idle' ? null : snapshot.activity.Running
  const blocked = pending || loading || running !== null || !open || !status
  const current = status?.document === 'Current'
  const canInitialize =
    status?.document === 'Absent' || status?.document === 'Unparsed'
  const parsed = results?.results ?? null
  const metadata = parsed?.metadata
  const documentLabel = !status
    ? '内容不可用'
    : {
        Absent: '待整理',
        Unparsed: '待解析',
        Current: '目录最新',
        Stale: '目录待更新'
      }[status.document]

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex shrink-0 flex-wrap items-center gap-2 px-3 py-2">
        {backLink}
        <div className="flex min-w-[min(100%,14rem)] flex-1 items-center gap-2">
          <h1
            className="min-w-0 truncate text-base font-semibold"
            title={snapshot.source}
          >
            {name}
          </h1>
          <span
            className={cn(
              'shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium',
              status?.document === 'Stale'
                ? 'bg-tertiary text-tertiary-foreground'
                : 'bg-secondary text-secondary-foreground'
            )}
          >
            {documentLabel}
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          {canInitialize && (
            <Button
              size="sm"
              onClick={() => void session.initialize()}
              disabled={blocked}
            >
              <FileTextIcon aria-hidden="true" />
              整理文本
            </Button>
          )}
          {windowed ? (
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="本书设置"
              title="本书设置"
              onClick={() => void openSettings()}
              disabled={!open}
            >
              <SettingsIcon aria-hidden="true" />
            </Button>
          ) : (
            <Button asChild variant="ghost" size="icon-sm">
              <Link
                to="/sessions/$sessionId/settings"
                params={{ sessionId: String(sessionId) }}
                aria-label="本书设置"
                title="本书设置"
              >
                <SettingsIcon aria-hidden="true" />
              </Link>
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="刷新状态"
            title="刷新状态"
            onClick={() => void session.refresh()}
            disabled={pending || loading || !open}
          >
            <RefreshIcon aria-hidden="true" />
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={() => {
              setTab('preview')
              void session.renderPreview()
            }}
            disabled={blocked || !current}
          >
            <EyeIcon aria-hidden="true" />
            生成预览
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void closeSession()}
            disabled={closing || !open}
          >
            <CloseIcon aria-hidden="true" />
            {closing || snapshot.lifecycle === 'Closing'
              ? '正在关闭…'
              : '关闭会话'}
          </Button>
        </div>
      </header>

      <MetadataBar
        key={`${status?.revision}-${!!results}`}
        parsed={metadata ?? null}
        overrides={results?.overrides ?? null}
        disabled={blocked || !results}
        onSave={(overrides) => void session.setOverrides(overrides)}
      >
        <form
          onSubmit={(event) => void exportBook(event)}
          className="flex min-w-0 flex-[2_1_18rem] items-center gap-1.5"
        >
          <label
            htmlFor="export-destination"
            className="shrink-0 text-xs text-muted-foreground"
          >
            保存到
          </label>
          <Input
            id="export-destination"
            value={destination}
            onChange={(event) => setDestination(event.target.value)}
            placeholder={windowed ? '导出时选择位置' : 'C:\\Books\\书名.epub'}
            autoComplete="off"
            spellCheck={false}
            disabled={blocked || !current}
            className="h-8 min-w-0 flex-1 rounded-lg px-2.5"
          />
          {windowed && (
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label="选择保存位置"
              title="选择保存位置"
              onClick={() => void chooseDestination()}
              disabled={blocked || !current}
            >
              <FolderIcon aria-hidden="true" />
            </Button>
          )}
          <Button
            type="submit"
            size="sm"
            disabled={blocked || !current || (!windowed && !destination.trim())}
          >
            <DownloadIcon aria-hidden="true" />
            导出 EPUB
          </Button>
        </form>
      </MetadataBar>

      <Messages
        closed={closed}
        backLink={backLink}
        connection={connection}
        onReconnect={session.reconnect}
        error={localError ?? error}
        notice={notice}
        exportPath={exportPath}
        warnings={warnings}
      />

      <div className="grid min-h-0 flex-1 grid-rows-[minmax(10rem,40%)_1fr] gap-2 px-2 pb-2 md:grid-cols-[minmax(15rem,20rem)_minmax(0,1fr)] md:grid-rows-1">
        <TocPanel
          toc={parsed?.toc ?? null}
          draft={draft?.toc ?? null}
          length={length}
          selected={selected?.id ?? null}
          onSelect={(row) => {
            setSelection({ id: row.id, title: row.title })
            setTab('text')
          }}
          onApplyDraft={() => draft && void session.install(draft)}
          onDiscardDraft={session.discardDraft}
          onRename={(row, title) => {
            session.editToc((toc) => renameTocEntry(toc, row.id, title))
            if (row.id === selected?.id) setSelection({ id: row.id, title })
          }}
          onRemove={(row) =>
            session.editToc((toc) => removeTocEntry(toc, row.id))
          }
          editable={!!draft || !!results?.current}
          disabled={blocked}
          emptyHint={
            closed
              ? '会话已关闭。'
              : canInitialize
                ? '整理文本后显示目录。'
                : '这份内容暂无目录条目。'
          }
        />

        <section className="flex min-h-0 flex-col overflow-hidden rounded-2xl bg-card">
          <div
            role="tablist"
            aria-label="工作区"
            className="flex h-10 shrink-0 items-end gap-1 border-b px-2"
          >
            {tabs.map((item) => (
              <button
                key={item.value}
                type="button"
                role="tab"
                id={`tab-${item.value}`}
                aria-selected={tab === item.value}
                aria-controls="workspace-panel"
                onClick={() => setTab(item.value)}
                className={cn(
                  'relative h-10 px-3 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground',
                  tab === item.value &&
                    'text-primary after:absolute after:inset-x-2 after:bottom-0 after:h-[3px] after:rounded-t-full after:bg-primary'
                )}
              >
                {item.label}
              </button>
            ))}
          </div>
          <div
            id="workspace-panel"
            role="tabpanel"
            aria-labelledby={`tab-${tab}`}
            className="flex min-h-0 flex-1 flex-col overflow-y-auto"
          >
            {tab === 'text' ? (
              <TextPanel
                row={selected}
                version={status?.document_version ?? null}
                readText={session.readText}
              />
            ) : tab === 'preview' ? (
              <PreviewPanel
                sessionId={sessionId}
                preview={preview}
                name={name}
                disabled={blocked}
                emptyHint={
                  closed
                    ? '本次会话已关闭，预览已失效。'
                    : current
                      ? '点击「生成预览」查看排版后的正文与目录。'
                      : '目录就绪后才能生成预览。'
                }
              />
            ) : tab === 'book' ? (
              <BookInfoPanel
                key={`${status?.revision}-${!!results}`}
                overrides={results?.overrides ?? null}
                disabled={blocked || !results}
                onSave={(overrides) => void session.setOverrides(overrides)}
              />
            ) : (
              session.settings && (
                <ParserPanel
                  settings={session.settings.toc}
                  disabled={blocked}
                  ready={!!status && status.document !== 'Absent'}
                  stale={status?.document === 'Stale'}
                  onParse={(toc) => void session.parse(toc)}
                />
              )
            )}
          </div>
        </section>
      </div>

      <StatusBar
        status={status ?? null}
        running={running}
        open={open}
        closed={closed}
        connection={connection}
        source={snapshot.source}
        cancelling={cancelling}
        onCancel={() => void cancelOperation()}
      />
    </div>
  )
}

function MetadataBar({
  parsed,
  overrides,
  disabled,
  onSave,
  children
}: {
  parsed: Metadata | null
  overrides: Metadata | null
  disabled: boolean
  onSave: (overrides: Metadata) => void
  children: ReactNode
}) {
  const [title, setTitle] = useState(overrides?.title ?? '')
  const [author, setAuthor] = useState(overrides?.author ?? '')

  // Empty fields fall back to the parsed values, which show as placeholders.
  function commit() {
    if (!overrides) return
    const next = {
      ...overrides,
      title: title.trim() || null,
      author: author.trim() || null
    }
    if (next.title !== overrides.title || next.author !== overrides.author) {
      onSave(next)
    }
  }

  const field = (
    id: string,
    label: string,
    value: string,
    set: (value: string) => void,
    fallback: string | null | undefined
  ) => (
    <div className="flex min-w-0 flex-[1_1_10rem] items-center gap-1.5">
      <label htmlFor={id} className="shrink-0 text-xs text-muted-foreground">
        {label}
      </label>
      <Input
        id={id}
        value={value}
        onChange={(event) => set(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') event.currentTarget.blur()
        }}
        placeholder={fallback || (parsed ? '未识别' : '整理后识别')}
        disabled={disabled}
        autoComplete="off"
        className="h-8 min-w-0 flex-1 rounded-lg px-2.5"
      />
    </div>
  )

  return (
    <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 px-3 pb-2">
      {field('book-title', '书名', title, setTitle, parsed?.title)}
      {field('book-author', '作者', author, setAuthor, parsed?.author)}
      {children}
    </div>
  )
}

function Messages({
  closed,
  backLink,
  connection,
  onReconnect,
  error,
  notice,
  exportPath,
  warnings
}: {
  closed: boolean
  backLink: ReactNode
  connection: string
  onReconnect: () => void
  error: string | null
  notice: string | null
  exportPath: string | null
  warnings: Array<{ path: string; message: string }>
}) {
  const strip = 'mx-2 mb-2 flex items-start gap-2 rounded-xl px-3 py-2 text-xs'
  return (
    <>
      {closed && (
        <div role="status" className={cn(strip, 'items-center bg-muted')}>
          <p className="flex-1">本次会话已结束，已导出的文件不受影响。</p>
          {backLink}
        </div>
      )}
      {connection === 'disconnected' && (
        <div role="status" className={cn(strip, 'items-center bg-muted')}>
          <p className="flex-1">实时连接已中断，显示的状态可能尚未更新。</p>
          <Button variant="outline" size="xs" onClick={onReconnect}>
            重新连接
          </Button>
        </div>
      )}
      {error && (
        <p
          role="alert"
          className={cn(
            strip,
            'break-words bg-destructive/10 text-destructive'
          )}
        >
          {error}
        </p>
      )}
      {exportPath && (
        <p
          role="status"
          className={cn(strip, 'bg-secondary text-secondary-foreground')}
        >
          <CheckIcon className="size-4 shrink-0" aria-hidden="true" />
          <span className="break-all">EPUB 已保存到 {exportPath}</span>
        </p>
      )}
      {notice && !exportPath && (
        <p
          role="status"
          className={cn(strip, 'bg-secondary text-secondary-foreground')}
        >
          {notice}
        </p>
      )}
      {warnings.length > 0 && (
        <div
          role="status"
          className={cn(strip, 'bg-tertiary text-tertiary-foreground')}
        >
          <WarningIcon className="size-4 shrink-0" aria-hidden="true" />
          <div className="min-w-0 space-y-1">
            <p className="font-medium">部分临时文件未能清理</p>
            <ul className="space-y-1">
              {warnings.map((warning, index) => (
                <li key={`${warning.path}-${index}`} className="break-all">
                  <p>{warning.message}</p>
                  <p className="opacity-80">{warning.path}</p>
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}
    </>
  )
}

type Running = Exclude<Activity, 'Idle'>['Running']

function StatusBar({
  status,
  running,
  open,
  closed,
  connection,
  source,
  cancelling,
  onCancel
}: {
  status: WorkspaceStatus | null
  running: Running | null
  open: boolean
  closed: boolean
  connection: string
  source: string
  cancelling: boolean
  onCancel: () => void
}) {
  const document = status?.document
  // Stages mirror the backend pipeline so a rerun of any stage shows here.
  const stages = [
    { label: '提取', done: !!document && document !== 'Absent' },
    ...(status && status.filters.total > 0
      ? [
          {
            label: `清理 ${status.filters.applied}/${status.filters.total}`,
            done: status.filters.applied === status.filters.total
          }
        ]
      : []),
    {
      label: document === 'Stale' ? '解析（已过期）' : '解析',
      done: document === 'Current'
    }
  ]
  const connectionLabel = {
    connecting: '连接中',
    live: '实时',
    disconnected: '已断开',
    closed: '已关闭'
  }[connection]

  return (
    <footer className="flex h-8 shrink-0 items-center gap-3 border-t px-3 text-[11px] text-muted-foreground">
      <ol aria-label="处理阶段" className="flex shrink-0 items-center gap-2">
        {stages.map((stage) => (
          <li key={stage.label} className="flex items-center gap-1">
            <span
              aria-hidden="true"
              className={cn(
                'size-1.5 rounded-full',
                stage.done ? 'bg-primary' : 'bg-muted-foreground/40'
              )}
            />
            <span className={stage.done ? 'text-foreground' : undefined}>
              {stage.label}
            </span>
          </li>
        ))}
      </ol>

      <div
        role="status"
        aria-live="polite"
        className="flex shrink-0 items-center gap-2"
      >
        {closed ? (
          <span>会话已关闭</span>
        ) : !open ? (
          <span>正在关闭会话…</span>
        ) : running ? (
          <>
            <span
              className="size-2 animate-pulse rounded-full bg-primary"
              aria-hidden="true"
            />
            <span className="text-foreground">
              {operationLabels[running.kind]}
              {running.cancel_requested
                ? ' · 正在取消'
                : phaseLabel(running.phase) &&
                  ` · ${phaseLabel(running.phase)}`}
            </span>
            <button
              type="button"
              onClick={onCancel}
              disabled={cancelling || running.cancel_requested}
              className="inline-flex items-center gap-0.5 rounded-full px-1.5 py-0.5 text-foreground hover:bg-muted disabled:opacity-50"
            >
              <StopIcon className="size-3.5" aria-hidden="true" />
              取消操作
            </button>
          </>
        ) : (
          <span>空闲</span>
        )}
      </div>

      <span className="min-w-0 flex-1 truncate text-right" title={source}>
        {source}
      </span>
      {status && (
        <span className="shrink-0 tabular-nums">r{status.revision}</span>
      )}
      <span className="shrink-0">{connectionLabel}</span>
    </footer>
  )
}
