import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import ImageIcon from '~icons/material-symbols/image-outline-rounded'
import PasteIcon from '~icons/material-symbols/content-paste-rounded'
import RemoveIcon from '~icons/material-symbols/hide-image-outline-rounded'
import type { CoverKind, CoverSettings } from '@/bindings'
import { Button } from '@/components/ui/button'
import { cn } from '@/utils/ui'
import { errorMessage, isBusy } from '../api'

export const coverKinds: Array<{ value: CoverKind; label: string }> = [
  { value: 'None', label: '无封面' },
  { value: 'Generated', label: '默认封面' },
  { value: 'Image', label: '自定义图片' }
]

type Props = {
  cover: CoverSettings
  hasImage: boolean
  /** Changes whenever the rendered cover may differ. */
  version: string
  disabled: boolean
  onChange: (cover: CoverSettings) => void
  onImage: (image: string | null) => void
  render: () => Promise<string | null>
  /** Extra image sources, such as a web search on desktop. */
  sources?: ReactNode
}

type Preview =
  { version: string; jpeg: string | null } | { version: string; error: string }

/** Reads `blob` as base64 without the `data:` prefix. */
export function readBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(String(reader.result).split(',', 2)[1] ?? '')
    reader.onerror = () => reject(reader.error ?? new Error('无法读取图片'))
    reader.readAsDataURL(blob)
  })
}

export function CoverEditor({
  cover,
  hasImage,
  version,
  disabled,
  onChange,
  onImage,
  render,
  sources
}: Props) {
  const [preview, setPreview] = useState<Preview | null>(null)
  const [problem, setProblem] = useState<string | null>(null)
  const file = useRef<HTMLInputElement>(null)
  // The latest props, so the paste listener need not be re-registered.
  const latest = useRef({ disabled, onImage })
  latest.current = { disabled, onImage }

  // Rendering is a session operation, so it waits until the session is
  // free. Each version is tried once: rendering marks the session busy, and
  // retrying whenever it is free again would loop on a failing cover. Only a
  // render rejected as busy is retried, when the session is free again.
  const attempted = useRef<string | null>(null)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => {
      mounted.current = false
    }
  }, [])
  useEffect(() => {
    if (disabled || attempted.current === version) return
    attempted.current = version
    render().then(
      (jpeg) => mounted.current && setPreview({ version, jpeg }),
      (cause: unknown) => {
        if (isBusy(cause)) {
          if (attempted.current === version) attempted.current = null
        } else if (mounted.current) {
          setPreview({ version, error: errorMessage(cause) })
        }
      }
    )
  }, [render, version, disabled])

  const applyBlob = useCallback(async (blob: Blob | undefined) => {
    setProblem(null)
    if (!blob) {
      setProblem('剪贴板里没有图片')
      return
    }
    try {
      latest.current.onImage(await readBase64(blob))
    } catch (cause) {
      setProblem(errorMessage(cause))
    }
  }, [])

  // Pasting anywhere on the page sets the cover, except into a text field.
  useEffect(() => {
    function paste(event: ClipboardEvent) {
      const target = event.target as HTMLElement | null
      if (
        latest.current.disabled ||
        target?.closest('input, textarea, [contenteditable="true"]')
      )
        return
      const image = [...(event.clipboardData?.items ?? [])].find((item) =>
        item.type.startsWith('image/')
      )
      if (!image) return
      event.preventDefault()
      void applyBlob(image.getAsFile() ?? undefined)
    }
    window.addEventListener('paste', paste)
    return () => window.removeEventListener('paste', paste)
  }, [applyBlob])

  async function pasteFromClipboard() {
    setProblem(null)
    try {
      const items = await navigator.clipboard.read()
      for (const item of items) {
        const type = item.types.find((type) => type.startsWith('image/'))
        if (type) return await applyBlob(await item.getType(type))
      }
      await applyBlob(undefined)
    } catch (cause) {
      setProblem(`无法读取剪贴板：${errorMessage(cause)}，可按 Ctrl+V 粘贴`)
    }
  }

  // The previous cover stays until the new one is ready, dimmed meanwhile.
  const updating = !!preview && preview.version !== version
  return (
    <section aria-labelledby="cover-heading" className="space-y-3">
      <h3 id="cover-heading" className="text-sm font-semibold">
        封面
      </h3>
      <div className="flex flex-wrap gap-4">
        <figure className="flex aspect-[2/3] w-40 shrink-0 items-center justify-center overflow-hidden rounded-xl bg-muted text-center text-xs text-muted-foreground">
          {!preview ? (
            <span>正在生成…</span>
          ) : 'error' in preview ? (
            <span
              role={updating ? undefined : 'alert'}
              className={cn('px-2 text-destructive', updating && 'opacity-50')}
            >
              {preview.error}
            </span>
          ) : preview.jpeg ? (
            <img
              src={`data:image/jpeg;base64,${preview.jpeg}`}
              alt="封面预览"
              aria-busy={updating}
              className={cn(
                'size-full object-contain transition-opacity',
                updating && 'opacity-60'
              )}
            />
          ) : (
            <span className={cn(updating && 'opacity-50')}>无封面</span>
          )}
        </figure>

        <div className="min-w-[14rem] flex-1 space-y-3">
          <div
            role="radiogroup"
            aria-label="封面来源"
            className="inline-flex rounded-full border border-input p-0.5"
          >
            {coverKinds.map((item) => (
              <button
                key={item.value}
                type="button"
                role="radio"
                aria-checked={cover.kind === item.value}
                disabled={disabled || (item.value === 'Image' && !hasImage)}
                onClick={() => onChange({ ...cover, kind: item.value })}
                className={cn(
                  'h-8 rounded-full px-3.5 text-xs font-medium transition-colors disabled:opacity-50',
                  cover.kind === item.value
                    ? 'bg-secondary text-secondary-foreground'
                    : 'text-muted-foreground hover:bg-muted'
                )}
              >
                {item.label}
              </button>
            ))}
          </div>

          <div className="flex flex-wrap gap-1.5">
            <Button
              size="sm"
              variant="outline"
              disabled={disabled}
              onClick={() => file.current?.click()}
            >
              <ImageIcon aria-hidden="true" />
              选择图片
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={disabled}
              onClick={() => void pasteFromClipboard()}
            >
              <PasteIcon aria-hidden="true" />
              粘贴图片
            </Button>
            {sources}
            {hasImage && (
              <Button
                size="sm"
                variant="ghost"
                disabled={disabled}
                onClick={() => onImage(null)}
              >
                <RemoveIcon aria-hidden="true" />
                移除图片
              </Button>
            )}
            <input
              ref={file}
              type="file"
              accept="image/*"
              aria-label="封面图片文件"
              className="hidden"
              onChange={(event) => {
                const chosen = event.target.files?.[0]
                event.target.value = ''
                if (chosen) void applyBlob(chosen)
              }}
            />
          </div>

          <CoverOptions cover={cover} disabled={disabled} onChange={onChange} />

          <p
            role={problem ? 'alert' : undefined}
            className={cn(
              'text-xs',
              problem ? 'text-destructive' : 'text-muted-foreground'
            )}
          >
            {problem ?? '也可以直接按 Ctrl+V 粘贴图片。'}
          </p>
        </div>
      </div>
    </section>
  )
}

export function CoverOptions({
  cover,
  disabled,
  onChange,
  defaults = false
}: {
  cover: CoverSettings
  disabled?: boolean
  onChange: (cover: CoverSettings) => void
  /** Edits the defaults for new books, which have no image yet. */
  defaults?: boolean
}) {
  const option = (
    key: 'overlay' | 'grayscale',
    label: string,
    hint: string,
    unavailable = false
  ) => (
    <label className="flex items-start gap-3">
      <input
        type="checkbox"
        checked={cover[key]}
        disabled={disabled || unavailable}
        onChange={(event) =>
          onChange({ ...cover, [key]: event.target.checked })
        }
        className="mt-0.5 size-4 accent-[var(--md-sys-color-primary)]"
      />
      <span className="space-y-0.5">
        <span className="block text-sm font-medium">{label}</span>
        <span className="block text-xs text-muted-foreground">{hint}</span>
      </span>
    </label>
  )
  return (
    <div className="space-y-2">
      {option(
        'overlay',
        '在图片上叠加书名和作者',
        '仅用于自定义图片；默认封面总会显示书名和作者。',
        !defaults && cover.kind !== 'Image'
      )}
      {option('grayscale', '黑白封面', '墨水屏阅读器上对比度更好。')}
    </div>
  )
}
