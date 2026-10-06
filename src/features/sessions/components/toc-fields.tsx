import type { ReactNode } from 'react'
import type { TocMode, VolumeSplit } from '@/bindings'
import { Input } from '@/components/ui/input'
import { cn } from '@/utils/ui'
import type { TocForm } from '../parser-config'

const modes: Array<{ value: TocMode; label: string; hint: string }> = [
  {
    value: 'VBook',
    label: '卷章',
    hint: '按章节标题切分，并识别分卷；首个分卷前的章节按数量自动成卷。'
  },
  { value: 'Chapters', label: '仅章节', hint: '只识别章节标题，不分卷。' },
  {
    value: 'Volumes',
    label: '卷 › 章',
    hint: '两级目录：分卷标题为一级，章节标题为二级。'
  },
  {
    value: 'Split',
    label: '均分',
    hint: '没有可识别的标题时，把全文按字节均分成若干段。'
  }
]

const volumeSplits: Array<{ value: VolumeSplit; label: string }> = [
  { value: 'Titles', label: '按分卷标题' },
  { value: 'Forced', label: '每 N 章一卷' },
  { value: 'None', label: '不分卷' }
]

type Props = {
  form: TocForm
  onChange: (patch: Partial<TocForm>) => void
}

export function TocFields({ form, onChange: update }: Props) {
  const mode = modes.find((item) => item.value === form.mode)!

  return (
    <div className="space-y-4">
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

      {form.mode === 'Split' ? (
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
              value={form.chapter_marks}
              onChange={(event) =>
                update({ chapter_marks: event.target.value })
              }
              className="h-9"
            />
          </Field>
          {form.mode !== 'Chapters' && (
            <Field label="分卷标记" hint="空格分隔，匹配“第一卷”和“卷一”">
              <Input
                value={form.volume_marks}
                onChange={(event) =>
                  update({ volume_marks: event.target.value })
                }
                className="h-9"
              />
            </Field>
          )}
          <Field label="标题最长字数" hint="更长的行不视为标题">
            <NumberInput
              value={form.max_title_len}
              onChange={(max_title_len) => update({ max_title_len })}
            />
          </Field>
          {form.mode === 'VBook' && (
            <>
              <Field label="分卷方式">
                <select
                  value={form.volume_split}
                  onChange={(event) =>
                    update({ volume_split: event.target.value as VolumeSplit })
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
              {form.volume_split !== 'None' && (
                <Field
                  label="每卷章数"
                  hint={
                    form.volume_split === 'Titles'
                      ? '用于首个分卷标题之前的章节'
                      : undefined
                  }
                >
                  <NumberInput
                    value={form.chapters_per_volume}
                    onChange={(chapters_per_volume) =>
                      update({ chapters_per_volume })
                    }
                  />
                </Field>
              )}
            </>
          )}
        </div>
      )}
    </div>
  )
}

export function Field({
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
