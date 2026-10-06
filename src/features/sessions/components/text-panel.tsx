import { useEffect, useState, type ReactNode } from 'react'
import type { DocumentVersion, TextRange } from '@/bindings'
import { errorMessage } from '../api'
import { formatBytes, type TocRow } from '../toc'

// The backend rejects larger reads, and arbitrary offsets may split a UTF-8
// character, so oversized chapters are not read in pieces.
const READ_LIMIT = 1024 * 1024

type Props = {
  row: TocRow | null
  version: DocumentVersion | null
  readText: (version: DocumentVersion, range: TextRange) => Promise<string>
}

type Loaded = { key: string; text?: string; error?: string }

export function TextPanel({ row, version, readText }: Props) {
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const span = row?.span ?? null
  const key =
    span && version
      ? `${version.document_id.join('.')}@${version.revision}:${span.start}-${span.end}`
      : null
  const readable = span !== null && span.end - span.start <= READ_LIMIT

  useEffect(() => {
    if (!key || !span || !version || !readable) return
    let active = true
    readText(version, span).then(
      (text) => active && setLoaded({ key, text }),
      (cause: unknown) =>
        active && setLoaded({ key, error: errorMessage(cause) })
    )
    return () => {
      active = false
    }
    // `key` captures every input that changes what is read.
  }, [key, readable])

  if (!row) {
    return <Hint>在目录中选择章节，查看它的正文。</Hint>
  }
  if (!span) {
    return (
      <Hint>
        {row.hasChildren
          ? '这是分卷标题，选择其中的章节查看正文。'
          : '无法确定这一章的结束位置（通常是最后一章），暂不能显示正文。'}
      </Hint>
    )
  }
  if (!readable) {
    return (
      <Hint>
        这一章有 {formatBytes(span.end - span.start)}，超过单次读取上限
        1M，请检查目录是否漏掉了章节标题。
      </Hint>
    )
  }
  const current = loaded?.key === key ? loaded : null
  return (
    <article className="mx-auto w-full max-w-3xl px-5 py-4">
      <p className="mb-3 text-xs tabular-nums text-muted-foreground">
        字节 {span.start.toLocaleString()} – {span.end.toLocaleString()} ·{' '}
        {formatBytes(span.end - span.start)}
      </p>
      {current?.error ? (
        <p role="alert" className="text-sm text-destructive">
          无法读取正文：{current.error}
        </p>
      ) : current?.text !== undefined ? (
        <pre className="whitespace-pre-wrap break-words font-[inherit] text-[15px] leading-7">
          {current.text}
        </pre>
      ) : (
        <p role="status" className="text-sm text-muted-foreground">
          正在读取正文…
        </p>
      )}
    </article>
  )
}

function Hint({ children }: { children: ReactNode }) {
  return (
    <p className="m-auto max-w-sm px-6 py-10 text-center text-sm leading-relaxed text-muted-foreground">
      {children}
    </p>
  )
}
