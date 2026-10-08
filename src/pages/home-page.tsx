import { useRef, useState, type FormEvent } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useNavigate } from '@tanstack/react-router'
import { motion, useReducedMotion } from 'motion/react'
import { M3eButton } from '@m3e/react/button'
import { M3eIconButton } from '@m3e/react/icon-button'
import { M3eList, M3eListAction } from '@m3e/react/list'
import ChevronIcon from '~icons/material-symbols/chevron-right-rounded'
import DescriptionIcon from '~icons/material-symbols/description-outline-rounded'
import NoteAddIcon from '~icons/material-symbols/note-add-outline-rounded'
import RefreshIcon from '~icons/material-symbols/refresh-rounded'
import UploadIcon from '~icons/material-symbols/upload-file-outline-rounded'
import { commands } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { errorMessage } from '@/features/sessions/api'
import { useSessions } from '@/features/sessions/use-sessions'
import { useFileDrop } from '@/hooks/use-file-drop'

export function HomePage() {
  const { sessions, loading, pending, error, refresh, create } = useSessions()
  const navigate = useNavigate()
  const reducedMotion = useReducedMotion()
  const [notice, setNotice] = useState<string | null>(null)
  const [source, setSource] = useState('')
  // create() rejects overlapping calls, so drops and picks wait their turn.
  const queue = useRef(Promise.resolve())
  // Desktop sessions each live in their own window; browsers have only this page.
  const windowed = isTauri()

  function addSources(paths: string[]) {
    setNotice(null)
    queue.current = queue.current.then(async () => {
      for (const path of paths) {
        const session = await create(path)
        if (session) await open(session.session)
      }
    })
  }

  const dragging = useFileDrop({
    onPaths: addSources,
    onUnsupported: () =>
      setNotice('浏览器无法读取拖入文件的路径，请在上方输入文件的完整路径。')
  })

  async function chooseFiles() {
    setNotice(null)
    try {
      const paths = await openDialog({
        multiple: true,
        directory: false,
        filters: [
          { name: '文本文件', extensions: ['txt'] },
          { name: '所有文件', extensions: ['*'] }
        ]
      })
      if (paths?.length) addSources(paths)
    } catch (cause) {
      setNotice(errorMessage(cause))
    }
  }

  function submitPath(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!source.trim()) return
    addSources([source.trim()])
    setSource('')
  }

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
    } catch (cause) {
      setNotice(errorMessage(cause))
    }
  }

  return (
    <motion.div
      className="mx-auto w-full max-w-5xl space-y-8 px-4 pb-10 pt-4 sm:px-6"
      initial={reducedMotion ? false : { opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.3 }}
    >
      <section
        aria-labelledby="home-heading"
        className="flex flex-col gap-6 rounded-[2rem] bg-primary-container p-6 text-primary-on-container sm:flex-row sm:items-center sm:justify-between sm:p-8"
      >
        <div className="space-y-2">
          <h1
            id="home-heading"
            className="text-3xl font-semibold tracking-tight"
          >
            开始一本新书
          </h1>
          <p className="max-w-md text-sm leading-relaxed opacity-80">
            {windowed
              ? '选择文本文件，或直接把文件拖进窗口，每个文件都会成为一个工作会话。'
              : '输入运行 WBook 的电脑上文本文件的完整路径。'}
          </p>
        </div>
        {windowed ? (
          <M3eButton
            variant="filled"
            size="medium"
            onClick={() => void chooseFiles()}
            disabled={pending}
          >
            <UploadIcon slot="icon" aria-hidden="true" />
            {pending ? '正在添加…' : '选择文件'}
          </M3eButton>
        ) : (
          <form
            onSubmit={submitPath}
            className="flex w-full gap-2 sm:max-w-sm"
            aria-busy={pending}
          >
            <Input
              aria-label="文本文件路径"
              value={source}
              onChange={(event) => setSource(event.target.value)}
              placeholder="例如 C:\Books\我的书.txt"
              autoComplete="off"
              spellCheck={false}
              className="bg-background"
            />
            <Button type="submit" disabled={pending || !source.trim()}>
              添加
            </Button>
          </form>
        )}
      </section>

      <section aria-labelledby="sessions-heading" className="space-y-3">
        <div className="flex items-center justify-between gap-4 px-2">
          <h2 id="sessions-heading" className="text-lg font-semibold">
            工作会话
          </h2>
          <M3eIconButton
            aria-label="刷新列表"
            title="刷新列表"
            onClick={() => void refresh()}
            disabled={loading || pending}
          >
            <RefreshIcon className="size-6" aria-hidden="true" />
          </M3eIconButton>
        </div>

        {(error || notice) && (
          <p
            role="alert"
            className="break-words rounded-2xl bg-destructive/10 p-4 text-sm text-destructive"
          >
            {notice ?? error}
          </p>
        )}

        {loading && sessions.length === 0 ? (
          <p
            role="status"
            className="rounded-[1.75rem] bg-card p-10 text-center text-sm text-muted-foreground"
          >
            正在读取工作会话…
          </p>
        ) : sessions.length === 0 && !error ? (
          <div className="flex flex-col items-center gap-3 rounded-[1.75rem] bg-card px-6 py-12 text-center">
            <div className="flex size-14 items-center justify-center rounded-2xl bg-secondary text-secondary-foreground">
              <NoteAddIcon className="size-7" aria-hidden="true" />
            </div>
            <h3 className="font-medium">还没有工作会话</h3>
            <p className="max-w-sm text-sm leading-relaxed text-muted-foreground">
              工作会话只在本次运行期间保留，关闭应用后会清空。
            </p>
          </div>
        ) : sessions.length === 0 ? null : (
          <M3eList
            variant="segmented"
            className="[--m3e-segmented-list-item-container-color:var(--md-sys-color-surface-container-low)]"
          >
            {sessions.map((session) => {
              const name = session.source.split(/[\\/]/).pop() || session.source
              const busy = session.activity !== 'Idle'
              return (
                <M3eListAction
                  key={session.session}
                  onClick={() => void open(session.session)}
                >
                  <span
                    slot="leading"
                    className="flex size-10 items-center justify-center rounded-xl bg-secondary text-secondary-foreground"
                  >
                    <DescriptionIcon className="size-6" aria-hidden="true" />
                  </span>
                  {name}
                  <span slot="supporting-text">
                    {session.lifecycle === 'Closing'
                      ? '正在关闭 · '
                      : busy
                        ? '正在处理 · '
                        : ''}
                    {session.source}
                  </span>
                  <ChevronIcon
                    slot="trailing"
                    className="size-6 shrink-0"
                    aria-hidden="true"
                  />
                </M3eListAction>
              )
            })}
          </M3eList>
        )}
      </section>

      {dragging && (
        <div
          role="status"
          className="pointer-events-none fixed inset-3 z-50 flex flex-col items-center justify-center gap-3 rounded-[2rem] border-2 border-dashed border-primary bg-primary-container/90 text-primary-on-container backdrop-blur-sm"
        >
          <UploadIcon className="size-12" aria-hidden="true" />
          <p className="text-xl font-semibold">松开以添加工作会话</p>
        </div>
      )}
    </motion.div>
  )
}
