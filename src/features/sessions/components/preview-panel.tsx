import { useEffect, useState } from 'react'
import type { PreviewInfo } from '@/bindings'
import { previewUrl } from '@/bridge'

type Props = {
  sessionId: number
  preview: PreviewInfo | null
  name: string
  disabled: boolean
  emptyHint: string
}

type Frame = { id: string; resource: string; url?: string; error?: string }

export function PreviewPanel({
  sessionId,
  preview,
  name,
  disabled,
  emptyHint
}: Props) {
  const [resource, setResource] = useState('')
  const [frame, setFrame] = useState<Frame | null>(null)
  const selected =
    preview && (preview.files.includes(resource) || resource === 'nav.xhtml')
      ? resource
      : preview?.files[0] || ''

  useEffect(() => {
    if (!preview || !selected) return
    let active = true
    void previewUrl(sessionId, preview, selected).then(
      (url) => active && setFrame({ id: preview.id, resource: selected, url }),
      (cause: unknown) =>
        active &&
        setFrame({
          id: preview.id,
          resource: selected,
          error: cause instanceof Error ? cause.message : String(cause)
        })
    )
    return () => {
      active = false
    }
  }, [preview, selected, sessionId])

  if (!preview) {
    return (
      <p className="m-auto max-w-sm px-6 py-10 text-center text-sm leading-relaxed text-muted-foreground">
        {emptyHint}
      </p>
    )
  }

  const current =
    frame?.id === preview.id && frame.resource === selected ? frame : null
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 p-2">
      <label className="flex shrink-0 items-center gap-2 px-1 text-xs text-muted-foreground">
        预览内容
        <select
          value={selected}
          disabled={disabled}
          onChange={(event) => setResource(event.target.value)}
          className="h-8 min-w-0 flex-1 rounded-lg border border-input bg-background px-2 text-sm text-foreground"
        >
          {preview.files.map((file, index) => (
            <option key={file} value={file}>
              {preview.files.length === 1 ? '完整正文' : `正文 ${index + 1}`}
            </option>
          ))}
          <option value="nav.xhtml">目录页</option>
        </select>
      </label>
      {current?.error ? (
        <p
          role="alert"
          className="rounded-xl bg-destructive/10 p-3 text-sm text-destructive"
        >
          无法打开预览：{current.error}
        </p>
      ) : current?.url ? (
        <iframe
          key={current.url}
          src={current.url}
          title={`${name} — ${selected === 'nav.xhtml' ? '目录' : '正文'}预览`}
          sandbox="allow-same-origin"
          referrerPolicy="no-referrer"
          className="min-h-0 w-full flex-1 rounded-xl border bg-white"
          onError={() =>
            setFrame({
              id: preview.id,
              resource: selected,
              error: '请稍后重新生成预览。'
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
    </div>
  )
}
