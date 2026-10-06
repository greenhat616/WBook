import { useState, type FormEvent } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { Link, useNavigate } from '@tanstack/react-router'
import { motion, useReducedMotion } from 'framer-motion'
import {
  ArrowRight,
  BookOpen,
  FileText,
  FolderOpen,
  Plus,
  RefreshCw
} from 'lucide-react'
import { commands } from '@/bindings'
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
import { errorMessage } from '@/features/sessions/api'
import { useSessions } from '@/features/sessions/use-sessions'

export function HomePage() {
  const { sessions, loading, pending, error, refresh, create } = useSessions()
  const navigate = useNavigate()
  const reducedMotion = useReducedMotion()
  const [source, setSource] = useState('')
  const [parts, setParts] = useState('1')
  const [formError, setFormError] = useState<string | null>(null)
  // Desktop sessions each live in their own window; browsers have only this page.
  const windowed = isTauri()

  async function open(sessionId: number) {
    try {
      if (windowed) {
        await commands.openSessionWindow(sessionId)
      } else {
        await navigate({
          to: '/sessions/$sessionId',
          params: { sessionId: String(sessionId) }
        })
      }
    } catch (error) {
      setFormError(errorMessage(error))
    }
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const count = Number(parts)
    if (!source.trim() || !Number.isSafeInteger(count) || count < 1) {
      setFormError('请填写文本文件的完整路径，并输入大于 0 的整数分段数。')
      return
    }
    setFormError(null)
    const session = await create(source.trim(), count)
    if (session) await open(session.session)
  }

  return (
    <motion.div
      className="space-y-10 sm:space-y-12"
      initial={reducedMotion ? false : { opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
    >
      <section
        aria-labelledby="home-heading"
        className="grid gap-8 lg:grid-cols-[1fr_1.1fr] lg:items-center"
      >
        <div className="space-y-6 py-2 sm:py-6">
          <Badge variant="secondary" className="gap-2 px-4 py-2">
            <BookOpen className="size-4" aria-hidden="true" />
            你的文字，本机成书
          </Badge>
          <h1
            id="home-heading"
            className="max-w-xl text-4xl font-semibold leading-tight tracking-tight sm:text-5xl lg:text-6xl"
          >
            从一份文字，
            <br />
            开始一本书。
          </h1>
          <p className="max-w-md text-base leading-relaxed text-muted-foreground sm:text-lg">
            导入文本、整理内容、检查预览，再带走一本 EPUB。
            让散落的文字，有一个适合阅读的模样。
          </p>
          <ol
            className="flex flex-wrap gap-x-5 gap-y-3 text-sm text-muted-foreground"
            aria-label="制作步骤"
          >
            {['导入文本', '整理与预览', '导出 EPUB'].map((step, index) => (
              <li key={step} className="flex items-center gap-2">
                <span
                  className="flex size-6 items-center justify-center rounded-full bg-secondary font-medium text-secondary-foreground"
                  aria-hidden="true"
                >
                  {index + 1}
                </span>
                {step}
              </li>
            ))}
          </ol>
        </div>

        <Card className="border-0 bg-secondary/60">
          <CardHeader>
            <div className="mb-3 flex size-12 items-center justify-center rounded-2xl bg-primary text-primary-foreground">
              <FolderOpen className="size-6" aria-hidden="true" />
            </div>
            <CardTitle className="text-2xl">开始整理</CardTitle>
            <CardDescription>
              选择这台电脑上的文本，创建一个工作会话。
            </CardDescription>
          </CardHeader>
          <CardContent>
            <form
              onSubmit={(event) => void submit(event)}
              className="space-y-5"
              aria-busy={pending}
            >
              <div className="space-y-2">
                <label htmlFor="source-path" className="text-sm font-medium">
                  文本文件路径
                </label>
                <Input
                  id="source-path"
                  value={source}
                  onChange={(event) => setSource(event.target.value)}
                  placeholder="例如 C:\Books\我的书.txt"
                  required
                  autoComplete="off"
                  spellCheck={false}
                  disabled={pending}
                  aria-describedby="source-help"
                />
                <p
                  id="source-help"
                  className="text-xs leading-relaxed text-muted-foreground"
                >
                  复制文本文件的完整路径。浏览器中也使用运行 WBook
                  的电脑上的文件。
                </p>
              </div>
              <div className="space-y-2">
                <label htmlFor="section-count" className="text-sm font-medium">
                  分段数量
                </label>
                <Input
                  id="section-count"
                  type="number"
                  inputMode="numeric"
                  min={1}
                  max={Number.MAX_SAFE_INTEGER}
                  step={1}
                  required
                  value={parts}
                  onChange={(event) => setParts(event.target.value)}
                  disabled={pending}
                  className="max-w-36"
                  aria-describedby="section-help"
                />
                <p
                  id="section-help"
                  className="text-xs leading-relaxed text-muted-foreground"
                >
                  按指定数量均分文本；先保留 1 段，也可以完整预览。
                </p>
              </div>
              {formError && (
                <p role="alert" className="text-sm text-destructive">
                  {formError}
                </p>
              )}
              <Button
                type="submit"
                size="lg"
                className="w-full sm:w-auto"
                disabled={pending}
              >
                <Plus className="size-4" aria-hidden="true" />
                {pending ? '正在创建…' : '创建工作会话'}
              </Button>
            </form>
          </CardContent>
        </Card>
      </section>

      <section aria-labelledby="sessions-heading" className="space-y-5">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <h2
              id="sessions-heading"
              className="text-2xl font-semibold tracking-tight"
            >
              当前工作台
            </h2>
            <p className="mt-1 text-sm text-muted-foreground">
              这里是本次运行中打开的内容，关闭应用后不会保留。
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void refresh()}
            disabled={loading || pending}
          >
            <RefreshCw className="size-4" aria-hidden="true" />
            {loading ? '正在刷新…' : '刷新列表'}
          </Button>
        </div>

        {error && (
          <div
            role="alert"
            className="rounded-2xl border border-destructive/20 bg-destructive/10 p-4 text-sm text-destructive"
          >
            <p className="font-medium">暂时无法完成操作</p>
            <p className="mt-1 break-words">{error}</p>
            <p className="mt-2">确认 WBook 正在运行，再刷新试试。</p>
          </div>
        )}

        {loading && sessions.length === 0 ? (
          <p
            role="status"
            className="rounded-3xl border border-dashed p-10 text-center text-sm text-muted-foreground"
          >
            正在读取工作会话…
          </p>
        ) : sessions.length === 0 && !error ? (
          <div className="flex flex-col items-center gap-3 rounded-3xl border border-dashed px-6 py-10 text-center">
            <div className="flex size-14 items-center justify-center rounded-full bg-secondary">
              <FileText className="size-6 text-primary" aria-hidden="true" />
            </div>
            <h3 className="font-medium">给下一本书留了位置</h3>
            <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
              从上面的表单导入文本，开始整理你的第一份内容。
            </p>
          </div>
        ) : (
          <ul className="grid gap-3 md:grid-cols-2">
            {sessions.map((session) => {
              const name = session.source.split(/[\\/]/).pop() || session.source
              const busy = session.activity !== 'Idle'
              return (
                <li key={session.session} className="min-w-0">
                  <Link
                    to="/sessions/$sessionId"
                    params={{ sessionId: String(session.session) }}
                    onClick={(event) => {
                      if (!windowed) return
                      event.preventDefault()
                      void open(session.session)
                    }}
                    className="group flex h-full items-start gap-4 rounded-3xl border bg-card p-5 transition-colors hover:bg-secondary/50 focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring"
                  >
                    <div className="flex size-11 shrink-0 items-center justify-center rounded-2xl bg-secondary text-primary">
                      <BookOpen className="size-5" aria-hidden="true" />
                    </div>
                    <div className="min-w-0 flex-1">
                      <h3 className="break-words font-medium">{name}</h3>
                      <p className="mt-1 break-all text-xs leading-relaxed text-muted-foreground">
                        {session.source}
                      </p>
                      <Badge variant="secondary" className="mt-3">
                        {session.lifecycle === 'Closing'
                          ? '正在关闭'
                          : busy
                            ? '正在处理'
                            : '打开工作会话'}
                      </Badge>
                    </div>
                    <ArrowRight
                      className="mt-1 size-4 shrink-0 text-muted-foreground"
                      aria-hidden="true"
                    />
                  </Link>
                </li>
              )
            })}
          </ul>
        )}
      </section>
    </motion.div>
  )
}
