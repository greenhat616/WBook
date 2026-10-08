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
    (form.render.language.trim() ? null : '语言不能为空') ??
    validateCoverSearch(form.cover_search) ??
    (form.export.location === 'Custom' && !form.export.custom_directory.trim()
      ? '请填写自定义导出目录'
      : null)
  )
}

function validateCoverSearch({
  query,
  engines
}: Settings['cover_search']): string | null {
  if (!query.trim()) return '封面搜索内容不能为空'
  for (const [index, engine] of engines.entries()) {
    const name = engine.name.trim() || `图片源 ${index + 1}`
    if (!engine.name.trim()) return `${name}需要名称`
    if (!/^https?:\/\//.test(engine.url) || !engine.url.includes('{query}'))
      return `${name}的网址需以 http(s):// 开头并包含 {query}`
  }
  return null
}

export function sameSettings(a: Settings, b: Settings): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}
