import { useState, type FormEvent, type ReactNode } from 'react'
import TuneIcon from '~icons/material-symbols/tune-rounded'
import type { TocParserConfig } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/utils/ui'
import {
  buildParserConfig,
  defaultParserForm,
  validateParserForm,
  type ParserForm,
  type ParserMode,
  type VolumeSplit
} from '../parser-config'

const modes: Array<{ value: ParserMode; label: string; hint: string }> = [
  {
    value: 'vbook',
    label: '卷章',
    hint: '按章节标题切分，并识别分卷；首个分卷前的章节按数量自动成卷。'
  },
  { value: 'chapters', label: '仅章节', hint: '只识别章节标题，不分卷。' },
  {
    value: 'volumes',
    label: '卷 › 章',
    hint: '两级目录：分卷标题为一级，章节标题为二级。'
  },
  {
    value: 'split',
    label: '均分',
    hint: '没有可识别的标题时，把全文按字节均分成若干段。'
  }
]

const volumeSplits: Array<{ value: VolumeSplit; label: string }> = [
  { value: 'titles', label: '按分卷标题' },
  { value: 'forced', label: '每 N 章一卷' },
  { value: 'none', label: '不分卷' }
]

type Props = {
  disabled: boolean
  // Parsing needs extracted text, which only initialization produces.
  ready: boolean
  stale: boolean
  onParse: (config: TocParserConfig) => void
}

export function ParserPanel({ disabled, ready, stale, onParse }: Props) {
  const [form, setForm] = useState<ParserForm>(defaultParserForm)
  const problem = validateParserForm(form)
  const update = (patch: Partial<ParserForm>) =>
    setForm((previous) => ({ ...previous, ...patch }))
  const mode = modes.find((item) => item.value === form.mode)!

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!problem) onParse(buildParserConfig(form))
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

      <fieldset className="space-y-2">
        <legend className="mb-2 text-xs font-medium text-muted-foreground">
          目录结构
        </legend>
        <div
          role="radiogroup"
          aria-label="目录结构"
          className="inline-flex rounded-full border border-input p-0.5"
        >
          {modes.map((item) => (
            <button
              key={item.value}
              type="button"
              role="radio"
              aria-checked={form.mode === item.value}
              onClick={() => update({ mode: item.value })}
              className={cn(
                'h-8 rounded-full px-3.5 text-xs font-medium transition-colors',
                form.mode === item.value
                  ? 'bg-secondary text-secondary-foreground'
                  : 'text-muted-foreground hover:bg-muted'
              )}
            >
              {item.label}
            </button>
          ))}
        </div>
        <p className="text-xs text-muted-foreground">{mode.hint}</p>
      </fieldset>

      {form.mode === 'split' ? (
        <Field label="分段数">
          <NumberInput
            value={form.parts}
            onChange={(parts) => update({ parts })}
          />
        </Field>
      ) : (
        <div className="grid gap-x-4 gap-y-3 sm:grid-cols-2">
          <Field label="章节标记" hint="空格分隔，匹配“第十章”和“章十”">
            <Input
              value={form.chapterMarks}
              onChange={(event) => update({ chapterMarks: event.target.value })}
              className="h-9"
            />
          </Field>
          {form.mode !== 'chapters' && (
            <Field label="分卷标记" hint="空格分隔，匹配“第一卷”和“卷一”">
              <Input
                value={form.volumeMarks}
                onChange={(event) =>
                  update({ volumeMarks: event.target.value })
                }
                className="h-9"
              />
            </Field>
          )}
          <Field label="标题最长字数" hint="更长的行不视为标题">
            <NumberInput
              value={form.maxTitleLength}
              onChange={(maxTitleLength) => update({ maxTitleLength })}
            />
          </Field>
          {form.mode === 'vbook' && (
            <>
              <Field label="分卷方式">
                <select
                  value={form.volumeSplit}
                  onChange={(event) =>
                    update({ volumeSplit: event.target.value as VolumeSplit })
                  }
                  className="h-9 w-full rounded-xl border border-input bg-background px-3"
                >
                  {volumeSplits.map((item) => (
                    <option key={item.value} value={item.value}>
                      {item.label}
                    </option>
                  ))}
                </select>
              </Field>
              {form.volumeSplit !== 'none' && (
                <Field
                  label="每卷章数"
                  hint={
                    form.volumeSplit === 'titles'
                      ? '用于首个分卷标题之前的章节'
                      : undefined
                  }
                >
                  <NumberInput
                    value={form.chaptersPerVolume}
                    onChange={(chaptersPerVolume) =>
                      update({ chaptersPerVolume })
                    }
                  />
                </Field>
              )}
            </>
          )}
        </div>
      )}

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
          onClick={() => setForm(defaultParserForm)}
        >
          恢复默认
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
              ? '试解析不会改动当前目录，确认结果后再应用。'
              : '先整理文本，才能按新规则解析。')}
        </p>
      </div>
    </form>
  )
}

function Field({
  label,
  hint,
  children
}: {
  label: string
  hint?: string
  children: ReactNode
}) {
  return (
    <label className="block space-y-1">
      <span className="text-xs font-medium text-muted-foreground">{label}</span>
      {children}
      {hint && (
        <span className="block text-[11px] text-muted-foreground">{hint}</span>
      )}
    </label>
  )
}

function NumberInput({
  value,
  onChange
}: {
  value: number
  onChange: (value: number) => void
}) {
  return (
    <Input
      type="number"
      inputMode="numeric"
      min={1}
      step={1}
      value={Number.isNaN(value) ? '' : value}
      onChange={(event) => onChange(event.target.valueAsNumber)}
      className="h-9 max-w-32"
    />
  )
}
