import { useState, type FormEvent } from 'react'
import TuneIcon from '~icons/material-symbols/tune-rounded'
import type { TocSettings } from '@/bindings'
import { Button } from '@/components/ui/button'
import { cn } from '@/utils/ui'
import {
  tocForm,
  tocSettings,
  validateTocForm,
  type TocForm
} from '../parser-config'
import { TocFields } from './toc-fields'

type Props = {
  // The session's saved rules; the form starts from them.
  settings: TocSettings
  disabled: boolean
  // Parsing needs extracted text, which only initialization produces.
  ready: boolean
  stale: boolean
  onParse: (settings: TocSettings) => void
}

export function ParserPanel({
  settings,
  disabled,
  ready,
  stale,
  onParse
}: Props) {
  const [form, setForm] = useState<TocForm>(() => tocForm(settings))
  const problem = validateTocForm(form)

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!problem) onParse(tocSettings(form))
  }

  return (
    <form
      onSubmit={submit}
      className="mx-auto w-full max-w-2xl space-y-4 px-4 py-4 text-sm"
    >
      {stale && (
        <p
          role="status"
          className="rounded-xl bg-tertiary px-3 py-2 text-xs text-tertiary-foreground"
        >
          正文在解析后被修改过，当前目录可能已不准确，建议重新解析。
        </p>
      )}

      <TocFields
        form={form}
        onChange={(patch) => setForm((previous) => ({ ...previous, ...patch }))}
      />

      <div className="flex flex-wrap items-center gap-3 border-t pt-4">
        <Button
          type="submit"
          size="sm"
          disabled={disabled || !ready || !!problem}
        >
          <TuneIcon aria-hidden="true" />
          试解析
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={() => setForm(tocForm(settings))}
        >
          撤销修改
        </Button>
        <p
          role={problem ? 'alert' : undefined}
          className={cn(
            'text-xs',
            problem ? 'text-destructive' : 'text-muted-foreground'
          )}
        >
          {problem ??
            (ready
              ? '试解析会把规则保存为本书设置，但不会改动当前目录，确认结果后再应用。'
              : '先整理文本，才能按新规则解析。')}
        </p>
      </div>
    </form>
  )
}
