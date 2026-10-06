import { useState, type FormEvent, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import SaveIcon from '~icons/material-symbols/save-outline-rounded'
import { queries, type Settings } from '@/bindings'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/features/sessions/api'
import { cn } from '@/utils/ui'
import { SettingsEditor } from './settings-editor'
import {
  fromSettingsForm,
  sameSettings,
  settingsForm,
  validateSettingsForm
} from './settings-form'

type Props = {
  saved: Settings
  disabled?: boolean
  // Shown above the editor, e.g. why the stored file was not used.
  children?: ReactNode
  onSave: (settings: Settings) => Promise<void>
  // Replaces the form with another source without saving it.
  restore: { label: string; load: () => Promise<Settings> }
}

export function SettingsScreen({
  saved,
  disabled = false,
  children,
  onSave,
  restore
}: Props) {
  const builtin = useQuery(queries.builtinTemplates())
  const [form, setForm] = useState(() => settingsForm(saved))
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const problem = validateSettingsForm(form)
  const dirty = problem !== null || !sameSettings(fromSettingsForm(form), saved)

  async function run(action: () => Promise<void>, done: string) {
    setPending(true)
    setError(null)
    setNotice(null)
    try {
      await action()
      setNotice(done)
    } catch (cause) {
      setError(errorMessage(cause))
    } finally {
      setPending(false)
    }
  }

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!problem) void run(() => onSave(fromSettingsForm(form)), '设置已保存')
  }

  return (
    <form onSubmit={submit} className="space-y-4">
      {children}
      <SettingsEditor
        form={form}
        onChange={(update) => {
          setForm(update)
          setNotice(null)
        }}
        builtin={builtin.data ?? null}
      />
      <div className="sticky bottom-0 flex flex-wrap items-center gap-2 rounded-[1.75rem] bg-surface-container-high p-3">
        <Button
          type="submit"
          size="sm"
          disabled={disabled || pending || !dirty || !!problem}
        >
          <SaveIcon aria-hidden="true" />
          {pending ? '正在保存…' : '保存'}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={pending || !dirty}
          onClick={() => {
            setForm(settingsForm(saved))
            setError(null)
            setNotice(null)
          }}
        >
          撤销修改
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={disabled || pending}
          onClick={() =>
            void run(async () => {
              setForm(settingsForm(await restore.load()))
            }, `已载入${restore.label}，保存后生效`)
          }
        >
          {restore.label}
        </Button>
        <p
          role={error || problem ? 'alert' : 'status'}
          className={cn(
            'min-w-0 break-words text-xs',
            error || problem ? 'text-destructive' : 'text-muted-foreground'
          )}
        >
          {error ?? problem ?? notice ?? (dirty ? '有未保存的修改' : '')}
        </p>
      </div>
    </form>
  )
}
