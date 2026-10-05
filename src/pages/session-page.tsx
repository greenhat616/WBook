import { useEffect, useState, type FormEvent } from 'react'
import { Link, useNavigate } from '@tanstack/react-router'
import { motion, useReducedMotion } from 'framer-motion'
import {
  ArrowLeft,
  BookOpen,
  Check,
  Download,
  Eye,
  FileText,
  RefreshCw,
  Square,
  X
} from 'lucide-react'
import type { OpKind, Phase, TocEntry } from '@/bindings'
import { previewUrl } from '@/bridge'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { useSession } from '@/features/sessions/use-session'

const operationLabels: Record<OpKind, string> = {
  Initialize: '整理文本',
  Parse: '识别内容',
  Install: '应用整理结果',
  Edit: '更新文本',
  SetMetadataOverrides: '更新书籍信息',
  ReadText: '读取文本',
  ReadResults: '读取整理结果',
  RenderPreview: '生成预览',
  ExportEpub: '导出 EPUB'
}

function phaseLabel(phase: Phase | null): string | null {
  if (phase === null) return null
  if (typeof phase === 'object') {
    return `清理内容 · 第 ${phase.Filtering.index + 1} / ${phase.Filtering.total} 步`
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

function TocList({ entries }: { entries: TocEntry[] }) {
  return (
    <ol className="space-y-2">
      {entries.map((entry) => (
        <li key={entry.id}>
          <p className="break-words rounded-xl bg-muted/60 px-3 py-2 text-sm leading-relaxed">
            {entry.title || '未命名章节'}
          </p>
          {entry.children.length > 0 && (
            <div className="mt-2 border-l pl-3">
              <TocList entries={entry.children} />
            </div>
          )}
        </li>
      ))}
    </ol>
  )
}

export function SessionPage({ sessionId }: { sessionId: number }) {
  const {
    snapshot,
    results,
    preview,
    error,
    warnings,
    loading,
    pending,
    connection,
    notice,
    exportPath,
    refresh,
    reconnect,
    initialize,
    renderPreview,
    exportBook,
    cancel,
    close
  } = useSession(sessionId)
  const navigate = useNavigate()
  const reducedMotion = useReducedMotion()
  const [destination, setDestination] = useState('')
  const [resource, setResource] = useState('')
  const [frame, setFrame] = useState<{
    id: string
    resource: string
    url: string
  } | null>(null)
  const [frameError, setFrameError] = useState<{
    id: string
    resource: string
    message: string
  } | null>(null)
  const [closing, setClosing] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const selectedResource =
    preview && (preview.files.includes(resource) || resource === 'nav.xhtml')
      ? resource
      : preview?.files[0] || ''

  useEffect(() => {
    let active = true
    if (preview && selectedResource) {
      void previewUrl(sessionId, preview, selectedResource).then(
        (url) => {
          if (active) {
            setFrame({ id: preview.id, resource: selectedResource, url })
            setFrameError(null)
          }
        },
        (cause: unknown) => {
          if (active) {
            setFrameError({
              id: preview.id,
              resource: selectedResource,
              message: cause instanceof Error ? cause.message : String(cause)
            })
          }
        }
      )
    }
    return () => {
      active = false
    }
  }, [preview, selectedResource, sessionId])

  async function closeSession() {
    setClosing(true)
    const report = await close()
    setClosing(false)
    if (report && report.cleanup_failures.length === 0)
      await navigate({ to: '/' })
  }

  async function cancelOperation() {
    setCancelling(true)
    await cancel()
    setCancelling(false)
  }

  function submitExport(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (destination.trim()) void exportBook(destination.trim())
  }

  const backLink = (
    <Link
      to="/"
      className="inline-flex items-center gap-2 rounded-full text-sm font-medium text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring"
    >
      <ArrowLeft className="size-4" aria-hidden="true" />
      返回工作台
    </Link>
  )

  if (!snapshot) {
    return (
      <div className="space-y-6">
        {backLink}
        <Card>
          <CardHeader>
            <CardTitle>
              {loading ? '正在打开工作会话…' : '暂时无法打开这份内容'}
            </CardTitle>
            <CardDescription>
              工作会话只在本次应用运行期间保留。
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {loading && (
              <p role="status" className="text-sm text-muted-foreground">
                正在读取内容状态。
              </p>
            )}
            {error && (
              <p role="alert" className="break-words text-sm text-destructive">
                {error}
              </p>
            )}
            {!loading && (
              <Button variant="outline" onClick={() => void refresh()}>
                <RefreshCw className="size-4" aria-hidden="true" />
                重新尝试
              </Button>
            )}
          </CardContent>
        </Card>
      </div>
    )
  }

  const name = snapshot.source.split(/[\\/]/).pop() || snapshot.source
  const status = snapshot.workspace_status.Available
  const closed = connection === 'closed' || snapshot.lifecycle === 'Closed'
  const open = snapshot.lifecycle === 'Open' && !closed
  const running =
    !open || snapshot.activity === 'Idle' ? null : snapshot.activity.Running
  const blocked = pending || loading || running !== null || !open || !status
  const current = status?.document === 'Current'
  const canInitialize =
    status?.document === 'Absent' || status?.document === 'Unparsed'
  const metadata = results?.results?.metadata
  const title = results?.overrides.title ?? metadata?.title
  const author = results?.overrides.author ?? metadata?.author
  const frameUrl =
    preview && frame?.id === preview.id && frame.resource === selectedResource
      ? frame.url
      : null
  const previewError =
    preview &&
    frameError?.id === preview.id &&
    frameError.resource === selectedResource
      ? frameError.message
      : null
  const documentLabel = !status
    ? '内容暂不可用'
    : {
        Absent: '等待整理',
        Unparsed: '等待整理',
        Current: '内容已就绪',
        Stale: '内容有更新'
      }[status.document]
  const connectionLabel = {
    connecting: '正在连接',
    live: '实时连接',
    disconnected: '连接已中断',
    closed: '会话已关闭'
  }[connection]

  return (
    <motion.div
      className="space-y-6"
      initial={reducedMotion ? false : { opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
    >
      {backLink}
      <header className="flex flex-wrap items-start justify-between gap-5">
        <div className="min-w-0 flex-1 space-y-2">
          <div className="flex flex-wrap items-center gap-2">
            <Badge variant="secondary">{documentLabel}</Badge>
            <Badge
              variant={connection === 'disconnected' ? 'outline' : 'secondary'}
            >
              {connectionLabel}
            </Badge>
          </div>
          <h1 className="break-words text-3xl font-semibold tracking-tight sm:text-4xl">
            {name}
          </h1>
          <p className="break-all text-xs leading-relaxed text-muted-foreground sm:text-sm">
            {snapshot.source}
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={() => void closeSession()}
          disabled={closing || !open}
        >
          <X className="size-4" aria-hidden="true" />
          {closing || snapshot.lifecycle === 'Closing'
            ? '正在关闭…'
            : '关闭会话'}
        </Button>
      </header>

      {connection === 'disconnected' && (
        <div className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border bg-secondary/50 p-4">
          <p role="status" className="text-sm">
            实时连接已中断，显示的状态可能尚未更新。
          </p>
          <Button variant="outline" size="sm" onClick={() => void reconnect()}>
            重新连接
          </Button>
        </div>
      )}
      {error && (
        <p
          role="alert"
          className="break-words rounded-2xl border border-destructive/20 bg-destructive/10 p-4 text-sm text-destructive"
        >
          {error}
        </p>
      )}
      {notice && (
        <p
          role="status"
          className="rounded-2xl bg-secondary px-4 py-3 text-sm text-secondary-foreground"
        >
          {notice}
        </p>
      )}
      {warnings.length > 0 && (
        <div
          role="status"
          className="space-y-2 rounded-2xl border border-amber-200 bg-amber-50 p-4 text-sm text-amber-950"
        >
          <p className="font-medium">部分临时文件未能清理</p>
          <ul className="space-y-2">
            {warnings.map((warning, index) => (
              <li key={`${warning.path}-${index}`} className="break-all">
                <p>{warning.message}</p>
                <p className="mt-1 text-xs">{warning.path}</p>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="flex flex-wrap items-center justify-between gap-4 rounded-3xl bg-secondary/70 p-5">
        <div role="status" aria-live="polite" className="min-w-0">
          <p className="font-medium">
            {closed
              ? '会话已关闭'
              : !open
                ? '正在关闭会话'
                : running
                  ? operationLabels[running.kind]
                  : documentLabel}
          </p>
          <p className="mt-1 text-sm text-muted-foreground">
            {closed
              ? '本次会话已结束，查看清理提示后可返回工作台。'
              : !open
                ? '正在结束当前操作并清理临时文件。'
                : running
                  ? running.cancel_requested
                    ? '正在等待当前操作取消。'
                    : phaseLabel(running.phase) || '正在处理，请稍候。'
                  : canInitialize
                    ? '先整理文本，生成目录和书籍信息。'
                    : current
                      ? '可以检查阅读预览，也可以直接导出 EPUB。'
                      : '刷新内容状态，或返回工作台重新导入文本。'}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {running ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => void cancelOperation()}
              disabled={cancelling || running.cancel_requested || !open}
            >
              <Square className="size-3" aria-hidden="true" />
              {cancelling || running.cancel_requested
                ? '正在取消…'
                : '取消操作'}
            </Button>
          ) : (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void refresh()}
              disabled={pending || loading || !open}
            >
              <RefreshCw className="size-4" aria-hidden="true" />
              刷新状态
            </Button>
          )}
          {canInitialize && (
            <Button onClick={() => void initialize()} disabled={blocked}>
              <FileText className="size-4" aria-hidden="true" />
              整理文本
            </Button>
          )}
        </div>
      </div>

      <div className="grid items-start gap-6 xl:grid-cols-[minmax(16rem,0.8fr)_minmax(0,1.7fr)]">
        <div className="min-w-0 space-y-6">
          <Card>
            <CardHeader>
              <CardTitle>书籍信息</CardTitle>
              <CardDescription>从文本中读取的标题与作者。</CardDescription>
            </CardHeader>
            <CardContent>
              <dl className="space-y-4 text-sm">
                <div>
                  <dt className="text-muted-foreground">书名</dt>
                  <dd className="mt-1 break-words font-medium">
                    {title || (results ? '未识别' : '整理后显示')}
                  </dd>
                </div>
                <div>
                  <dt className="text-muted-foreground">作者</dt>
                  <dd className="mt-1 break-words font-medium">
                    {author || (results ? '未识别' : '整理后显示')}
                  </dd>
                </div>
              </dl>
            </CardContent>
          </Card>
          <Card>
            <CardHeader>
              <CardTitle>内容目录</CardTitle>
              <CardDescription>
                {results?.current
                  ? '已整理的阅读顺序。'
                  : '完成整理后，在这里查看内容结构。'}
              </CardDescription>
            </CardHeader>
            <CardContent>
              {results?.results?.toc.length ? (
                <div className="max-h-96 overflow-y-auto pr-1">
                  <TocList entries={results.results.toc} />
                </div>
              ) : (
                <p className="text-sm text-muted-foreground">
                  {results ? '这份内容暂无目录条目。' : '目录还没有生成。'}
                </p>
              )}
            </CardContent>
          </Card>
        </div>

        <Card className="min-w-0 overflow-hidden">
          <CardHeader className="flex flex-wrap flex-row items-start justify-between gap-4">
            <div className="space-y-1.5">
              <CardTitle>阅读预览</CardTitle>
              <CardDescription>看看文字在书页中的样子。</CardDescription>
            </div>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => void renderPreview()}
              disabled={blocked || !current}
            >
              <Eye className="size-4" aria-hidden="true" />
              {preview ? '重新生成预览' : '生成预览'}
            </Button>
          </CardHeader>
          <CardContent className="space-y-4">
            {preview ? (
              <>
                <div className="space-y-2">
                  <label
                    htmlFor="preview-section"
                    className="text-sm font-medium"
                  >
                    预览内容
                  </label>
                  <select
                    id="preview-section"
                    value={selectedResource}
                    disabled={blocked}
                    onChange={(event) => setResource(event.target.value)}
                    className="flex min-h-11 w-full rounded-xl border border-input bg-background px-3 py-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {preview.files.map((file, index) => (
                      <option key={file} value={file}>
                        {preview.files.length === 1
                          ? '完整正文'
                          : `正文 ${index + 1}`}
                      </option>
                    ))}
                    <option value="nav.xhtml">目录页</option>
                  </select>
                </div>
                {previewError ? (
                  <p
                    role="alert"
                    className="break-words rounded-2xl bg-destructive/10 p-4 text-sm text-destructive"
                  >
                    无法打开预览：{previewError}
                  </p>
                ) : frameUrl ? (
                  <iframe
                    key={frameUrl}
                    src={frameUrl}
                    title={`${name} — ${selectedResource === 'nav.xhtml' ? '目录' : '正文'}预览`}
                    sandbox="allow-same-origin"
                    referrerPolicy="no-referrer"
                    className="h-[34rem] w-full rounded-2xl border bg-white sm:h-[40rem]"
                    onError={() =>
                      setFrameError({
                        id: preview.id,
                        resource: selectedResource,
                        message: '请稍后重新生成预览。'
                      })
                    }
                  />
                ) : (
                  <p
                    role="status"
                    className="py-10 text-center text-sm text-muted-foreground"
                  >
                    正在打开预览…
                  </p>
                )}
              </>
            ) : (
              <div className="flex min-h-72 flex-col items-center justify-center gap-4 rounded-2xl border border-dashed bg-muted/30 px-6 py-10 text-center">
                <div className="flex size-16 items-center justify-center rounded-3xl bg-secondary">
                  <BookOpen
                    className="size-7 text-primary"
                    aria-hidden="true"
                  />
                </div>
                <p className="font-medium">留一页，看看成书的样子</p>
                <p className="max-w-xs text-sm leading-relaxed text-muted-foreground">
                  {closed
                    ? '本次会话已关闭，预览已失效。'
                    : current
                      ? '点击「生成预览」，检查整理后的正文与目录。'
                      : '先完成文本整理，再生成阅读预览。'}
                </p>
              </div>
            )}
          </CardContent>
        </Card>
      </div>

      <Card className="border-0 bg-secondary/50">
        <CardHeader>
          <CardTitle>把这本书带走</CardTitle>
          <CardDescription>
            {closed
              ? '本次会话已结束，已导出的文件不受影响。'
              : '导出为 EPUB，可以在支持的阅读器中打开。'}
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={submitExport} className="space-y-3">
            <label htmlFor="export-destination" className="text-sm font-medium">
              保存路径
            </label>
            <div className="flex flex-col gap-3 sm:flex-row">
              <Input
                id="export-destination"
                value={destination}
                onChange={(event) => setDestination(event.target.value)}
                placeholder="例如 C:\Books\我的书.epub"
                required
                autoComplete="off"
                spellCheck={false}
                disabled={blocked || !current}
                aria-describedby="export-help"
                className="min-w-0 flex-1"
              />
              <Button
                type="submit"
                disabled={blocked || !current || !destination.trim()}
              >
                <Download className="size-4" aria-hidden="true" />
                导出 EPUB
              </Button>
            </div>
            <p
              id="export-help"
              className="text-xs leading-relaxed text-muted-foreground"
            >
              输入完整文件路径；文件夹需要已存在，请使用一个新的文件名。
            </p>
          </form>
          {exportPath && (
            <div
              role="status"
              className="mt-5 flex items-start gap-3 rounded-2xl bg-background/70 p-4"
            >
              <Check
                className="mt-0.5 size-5 shrink-0 text-primary"
                aria-hidden="true"
              />
              <div className="min-w-0">
                <p className="text-sm font-medium">EPUB 已保存</p>
                <p className="mt-1 break-all text-sm text-muted-foreground">
                  {exportPath}
                </p>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </motion.div>
  )
}
