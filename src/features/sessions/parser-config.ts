import type {
  HeadingRuleConfig,
  SimpleRuleConfig,
  TocParserConfig,
  VolumeMode
} from '@/bindings'

// Mirrors backend/wbook-core/src/parser/toc/presets.rs; keep the two in sync.
const EXTRA_PATTERN =
  '^\\s*(简介|序言|序曲|楔子|前言|后记|尾声|番外[^\\n]{0,25})$'

export type ParserMode = 'vbook' | 'chapters' | 'volumes' | 'split'
export type VolumeSplit = 'titles' | 'none' | 'forced'

export type ParserForm = {
  mode: ParserMode
  chapterMarks: string
  volumeMarks: string
  maxTitleLength: number
  volumeSplit: VolumeSplit
  // VBook groups chapters before the first volume heading into volumes of this size.
  chaptersPerVolume: number
  parts: number
}

export const defaultParserForm: ParserForm = {
  mode: 'vbook',
  chapterMarks: '章 回 节 集',
  volumeMarks: '部 卷',
  maxTitleLength: 25,
  volumeSplit: 'titles',
  chaptersPerVolume: 50,
  parts: 10
}

function marks(value: string): string[] {
  return value.split(/\s+/).filter(Boolean)
}

function simpleRule(
  prefixes: string[],
  suffixes: string[],
  maxTitleLength: number
): HeadingRuleConfig {
  const rule: SimpleRuleConfig = {
    allow_leading_space: true,
    prefixes,
    numeral: 'Mixed',
    suffixes,
    min_numeral_len: 1,
    max_numeral_len: 9,
    max_title_len: maxTitleLength
  }
  return { Simple: rule }
}

// "第十章" and the prefix-only "章十" forms, as in the backend presets.
function headingRules(list: string[], maxTitleLength: number) {
  return [
    simpleRule(['第'], list, maxTitleLength),
    simpleRule(list, [], maxTitleLength)
  ]
}

const extraRule: HeadingRuleConfig = {
  Regex: { pattern: EXTRA_PATTERN, title_group: null }
}

/** Returns a validation message, or null when the backend will accept the form. */
export function validateParserForm(form: ParserForm): string | null {
  const positive = (value: number) => Number.isSafeInteger(value) && value > 0
  if (form.mode === 'split') {
    return positive(form.parts) ? null : '分段数必须是正整数'
  }
  if (!marks(form.chapterMarks).length) return '至少需要一个章节标记'
  if (form.mode !== 'chapters' && !marks(form.volumeMarks).length) {
    return '至少需要一个分卷标记'
  }
  if (!positive(form.maxTitleLength)) return '标题长度上限必须是正整数'
  if (
    form.mode === 'vbook' &&
    form.volumeSplit !== 'none' &&
    !positive(form.chaptersPerVolume)
  ) {
    return '每卷章数必须是正整数'
  }
  return null
}

export function buildParserConfig(form: ParserForm): TocParserConfig {
  const chapters = headingRules(marks(form.chapterMarks), form.maxTitleLength)
  const volumes = headingRules(marks(form.volumeMarks), form.maxTitleLength)
  switch (form.mode) {
    case 'split':
      return { SplitEvenly: { parts: form.parts } }
    case 'chapters':
      return {
        Levels: { levels: [{ level: 1, rules: [...chapters, extraRule] }] }
      }
    case 'volumes':
      return {
        Levels: {
          levels: [
            { level: 1, rules: [...volumes, extraRule] },
            { level: 2, rules: chapters }
          ]
        }
      }
    case 'vbook': {
      const volumeMode: VolumeMode =
        form.volumeSplit === 'none'
          ? 'None'
          : form.volumeSplit === 'forced'
            ? { Forced: { chapters_per_volume: form.chaptersPerVolume } }
            : {
                Normal: {
                  rules: volumes,
                  fallback_chapters_per_volume: form.chaptersPerVolume
                }
              }
      return { VBook: { chapters: { Rules: chapters }, volumes: volumeMode } }
    }
  }
}

export const defaultParserConfig = buildParserConfig(defaultParserForm)
