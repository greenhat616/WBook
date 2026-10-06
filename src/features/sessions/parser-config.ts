import type { TocSettings } from '@/bindings'

// Marks are edited as space-separated text; the backend stores lists.
export type TocForm = Omit<TocSettings, 'chapter_marks' | 'volume_marks'> & {
  chapter_marks: string
  volume_marks: string
}

function marks(value: string): string[] {
  return value.split(/\s+/).filter(Boolean)
}

export function tocForm(settings: TocSettings): TocForm {
  return {
    ...settings,
    chapter_marks: settings.chapter_marks.join(' '),
    volume_marks: settings.volume_marks.join(' ')
  }
}

export function tocSettings(form: TocForm): TocSettings {
  return {
    ...form,
    chapter_marks: marks(form.chapter_marks),
    volume_marks: marks(form.volume_marks)
  }
}

/**
 * Returns a validation message, or null when the backend will accept the
 * form. Mirrors `TocSettings::to_config` so mistakes show before saving.
 */
export function validateTocForm(form: TocForm): string | null {
  const positive = (value: number) => Number.isSafeInteger(value) && value > 0
  if (form.mode === 'Split') {
    return positive(form.parts) ? null : '分段数必须是正整数'
  }
  if (!marks(form.chapter_marks).length) return '至少需要一个章节标记'
  const needsVolumeMarks =
    form.mode === 'Volumes' ||
    (form.mode === 'VBook' && form.volume_split === 'Titles')
  if (needsVolumeMarks && !marks(form.volume_marks).length) {
    return '至少需要一个分卷标记'
  }
  if (!positive(form.max_title_len)) return '标题长度上限必须是正整数'
  if (
    form.mode === 'VBook' &&
    form.volume_split !== 'None' &&
    !positive(form.chapters_per_volume)
  ) {
    return '每卷章数必须是正整数'
  }
  return null
}

export function sameToc(a: TocSettings, b: TocSettings): boolean {
  return JSON.stringify(a) === JSON.stringify(b)
}
