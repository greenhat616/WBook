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
  validateSettingsForm,
  type SettingsForm
} from './settings-form'
import { useSyncedForm } from './use-synced-form'

const codec = {
  toForm: settingsForm,
  read: (form: SettingsForm) =>
    validateSettingsForm(form) ? null : fromSettingsForm(form),
  same: sameSettings
}

type Props = {
  saved: Settings
  disabled?: boolean
  // Shown above the editor, e.g. why the stored file was not used.
  children?: ReactNode
  onSave: (settings: Settings) => Promise<void>
  // Replaces the form with another source without saving it.
  restore: { label: string; load: () => Promise<Settings> }
  /** Shows the settings that only the global settings use. */
  global?: boolean
}

export function SettingsScreen({
  saved,
  disabled = false,
  children,
  onSave,
  restore,
  global = false
}: Props) {
  const builtin = useQuery(queries.builtinTemplates())
  const synced = useSyncedForm(saved, codec)
  const { form, setForm, dirty, outdated } = synced
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const problem = validateSettingsForm(form)

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
    if (problem || outdated) return
    const next = fromSettingsForm(form)
    void run(async () => {
      await onSave(next)
      synced.committed(next)
    }, '设置已保存')
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
        global={global}
      />
      <div className="sticky bottom-0 flex flex-wrap items-center gap-2 rounded-[1.75rem] bg-surface-container-high p-3">
        <Button
          type="submit"
          size="sm"
          disabled={disabled || pending || !dirty || !!problem || outdated}
        >
          <SaveIcon aria-hidden="true" />
          {pending ? '正在保存…' : '保存'}
        </Button>
        <Button
          type="button"
          size="sm"
          variant="ghost"
          disabled={pending || (!dirty && !outdated)}
          onClick={() => {
            synced.reset()
            setError(null)
            setNotice(null)
          }}
        >
          {outdated ? '载入最新' : '撤销修改'}
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
          role={error || problem || outdated ? 'alert' : 'status'}
          className={cn(
            'min-w-0 break-words text-xs',
            error || problem || outdated
              ? 'text-destructive'
              : 'text-muted-foreground'
          )}
        >
          {error ??
            problem ??
            (outdated
              ? '设置已在其他窗口更新；载入最新后再修改'
              : (notice ?? (dirty ? '有未保存的修改' : '')))}
        </p>
      </div>
    </form>
  )
}
