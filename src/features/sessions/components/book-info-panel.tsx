import { useState, type FormEvent } from 'react'
import SaveIcon from '~icons/material-symbols/save-outline-rounded'
import type { Metadata } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Field } from './toc-fields'

type Fields = Pick<Metadata, 'isbn' | 'publisher' | 'published' | 'description'>
const keys = ['isbn', 'publisher', 'published', 'description'] as const

type Props = {
  overrides: Metadata | null
  disabled: boolean
  onSave: (overrides: Metadata) => void
}

/** Publication details that only matter for the packaged EPUB. */
export function BookInfoPanel({ overrides, disabled, onSave }: Props) {
  const saved = Object.fromEntries(
    keys.map((key) => [key, overrides?.[key] ?? ''])
  ) as Record<keyof Fields, string>
  const [form, setForm] = useState(saved)
  const changed = keys.some((key) => form[key].trim() !== saved[key])

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!overrides || !changed) return
    const next = { ...overrides }
    for (const key of keys) next[key] = form[key].trim() || null
    onSave(next)
  }

  const input = (key: keyof Fields) => ({
    value: form[key],
    onChange: (event: { target: { value: string } }) =>
      setForm((previous) => ({ ...previous, [key]: event.target.value })),
    disabled,
    autoComplete: 'off'
  })

  return (
    <div className="mx-auto w-full max-w-2xl space-y-6 px-4 py-4 text-sm">
      <form onSubmit={submit} className="space-y-3">
        <h3 className="text-sm font-semibold">出版信息</h3>
        <div className="grid gap-x-4 gap-y-3 sm:grid-cols-2">
          <Field label="ISBN" hint="ISBN-10 或 ISBN-13，可带连字符">
            <Input {...input('isbn')} className="h-9" spellCheck={false} />
          </Field>
          <Field label="出版社">
            <Input {...input('publisher')} className="h-9" />
          </Field>
          <Field label="出版日期" hint="例如 2024、2024-05 或 2024-05-01">
            <Input {...input('published')} className="h-9" spellCheck={false} />
          </Field>
        </div>
        <Field label="简介">
          <textarea
            {...input('description')}
            rows={5}
            className="w-full rounded-xl border border-input bg-background/70 px-3 py-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/20 disabled:opacity-50"
          />
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" size="sm" disabled={disabled || !changed}>
            <SaveIcon aria-hidden="true" />
            保存出版信息
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={!changed}
            onClick={() => setForm(saved)}
          >
            撤销修改
          </Button>
        </div>
      </form>
    </div>
  )
}
