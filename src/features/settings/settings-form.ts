import type { Settings } from '@/bindings'
import {
  tocForm,
  tocSettings,
  validateTocForm,
  type TocForm
} from '@/features/sessions/parser-config'

export type SettingsForm = Omit<Settings, 'toc'> & { toc: TocForm }

export function settingsForm(settings: Settings): SettingsForm {
  return { ...settings, toc: tocForm(settings.toc) }
}

export function fromSettingsForm(form: SettingsForm): Settings {
  return { ...form, toc: tocSettings(form.toc) }
}

// Templates and the language tag are checked by the backend on save.
export function validateSettingsForm(form: SettingsForm): string | null {
  return (
    validateTocForm(form.toc) ??
    (form.render.language.trim() ? null : '语言不能为空')
  )
}

export function sameSettings(a: Settings, b: Settings): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}
