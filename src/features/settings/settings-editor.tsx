import type { ReactNode } from 'react'
import type {
  CoverSearchSettings,
  CoverSettings,
  SearchEngine,
  RenderLayout,
  TemplateOverrides,
  RenderSettings
} from '@/bindings'
import AddIcon from '~icons/material-symbols/add-rounded'
import DeleteIcon from '~icons/material-symbols/delete-outline-rounded'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  CoverOptions,
  coverKinds
} from '@/features/sessions/components/cover-editor'
import { Field, TocFields } from '@/features/sessions/components/toc-fields'
import { cn } from '@/utils/ui'
import type { SettingsForm } from './settings-form'

const layouts: Array<{ value: RenderLayout; label: string }> = [
  { value: 'SingleHtml', label: '全书一个文件' },
  { value: 'SplitChapters', label: '每章一个文件' },
  { value: 'Paged', label: '一个文件，章节分页' }
]

const templates: Array<{
  key: keyof TemplateOverrides
  label: string
  hint: string
}> = [
  {
    key: 'stylesheet',
    label: '样式表 book.css',
    hint: '所有正文页与目录页共用的 CSS。'
  },
  {
    key: 'document',
    label: '页面开头 document.xhtml',
    hint: '每个正文文件和目录页从 XML 声明到 <body> 的部分。可用 book.title、book.author、book.language；navigation 为 true 时是位于包根目录的目录页，样式表路径需随之调整。'
  },
  {
    key: 'section',
    label: '章节开头 section.xhtml',
    hint: '每个章节的开头，</section> 由程序补上。可用 id、title、depth、heading（1–6）和 paged（分页布局中非首章为 true）。'
  },
  {
    key: 'paragraph',
    label: '段落 paragraph.xhtml',
    hint: '正文的每一行。可用 text 与 empty（空行为 true）。内容会自动转义。'
  }
]

type Props = {
  form: SettingsForm
  // Takes an updater so several edits in one event cannot overwrite each other.
  onChange: (update: (form: SettingsForm) => SettingsForm) => void
  // Built-in sources seed a template when the user starts overriding it.
  builtin: TemplateOverrides | null
  global?: boolean
}

export function SettingsEditor({
  form,
  onChange,
  builtin,
  global = false
}: Props) {
  const search = (patch: Partial<CoverSearchSettings>) =>
    onChange((form) => ({
      ...form,
      cover_search: { ...form.cover_search, ...patch }
    }))
  const engine = (index: number, patch: Partial<SearchEngine> | null) =>
    onChange((form) => ({
      ...form,
      cover_search: {
        ...form.cover_search,
        engines: form.cover_search.engines.flatMap((item, at) =>
          at !== index ? [item] : patch ? [{ ...item, ...patch }] : []
        )
      }
    }))
  const render = (patch: Partial<RenderSettings>) =>
    onChange((form) => ({ ...form, render: { ...form.render, ...patch } }))
  const template = (key: keyof TemplateOverrides, value: string | null) =>
    onChange((form) => ({
      ...form,
      render: {
        ...form.render,
        templates: { ...form.render.templates, [key]: value }
      }
    }))
  const cover = (cover: CoverSettings) =>
    onChange((form) => ({ ...form, cover }))
  const adFilter = form.filters.includes('Ad')

  return (
    <div className="space-y-4">
      <Section title="目录解析" description="章节与分卷标题的识别方式。">
        <TocFields
          form={form.toc}
          onChange={(patch) =>
            onChange((form) => ({ ...form, toc: { ...form.toc, ...patch } }))
          }
        />
      </Section>

      <Section title="文本清理" description="只在整理文本时运行一次。">
        <label className="flex items-start gap-3">
          <input
            type="checkbox"
            checked={adFilter}
            onChange={(event) => {
              const checked = event.target.checked
              onChange((form) => ({
                ...form,
                filters: checked
                  ? [...form.filters, 'Ad']
                  : form.filters.filter((filter) => filter !== 'Ad')
              }))
            }}
            className="mt-0.5 size-4 accent-[var(--md-sys-color-primary)]"
          />
          <span className="space-y-0.5">
            <span className="block text-sm font-medium">清除广告行</span>
            <span className="block text-xs text-muted-foreground">
              删除含下载站推广词或网址的整行。
            </span>
          </span>
        </label>
      </Section>

      <Section title="渲染" description="预览与导出 EPUB 的排版方式。">
        <div className="grid gap-x-4 gap-y-3 sm:grid-cols-2">
          <Field label="文件布局">
            <select
              value={form.render.layout}
              onChange={(event) =>
                render({ layout: event.target.value as RenderLayout })
              }
              className="h-9 w-full rounded-xl border border-input bg-background px-3"
            >
              {layouts.map((item) => (
                <option key={item.value} value={item.value}>
                  {item.label}
                </option>
              ))}
            </select>
          </Field>
          <Field label="语言" hint="BCP 47 语言标签，例如 zh-CN、zh-Hant">
            <Input
              value={form.render.language}
              onChange={(event) => render({ language: event.target.value })}
              autoComplete="off"
              spellCheck={false}
              className="h-9 max-w-40"
            />
          </Field>
        </div>
      </Section>

      <Section
        title="封面"
        description="自定义图片在工作区的「书籍信息」中选择；这里的设置也是新书的默认值。"
      >
        <div
          role="radiogroup"
          aria-label="封面来源"
          className="inline-flex rounded-full border border-input p-0.5"
        >
          {coverKinds
            // A book picks its image in the workspace, which sets this.
            .filter(
              (item) => item.value !== 'Image' || form.cover.kind === 'Image'
            )
            .map((item) => (
              <button
                key={item.value}
                type="button"
                role="radio"
                aria-checked={form.cover.kind === item.value}
                onClick={() => cover({ ...form.cover, kind: item.value })}
                className={cn(
                  'h-8 rounded-full px-3.5 text-xs font-medium transition-colors',
                  form.cover.kind === item.value
                    ? 'bg-secondary text-secondary-foreground'
                    : 'text-muted-foreground hover:bg-muted'
                )}
              >
                {item.label}
              </button>
            ))}
        </div>
        <CoverOptions cover={form.cover} onChange={cover} defaults />
      </Section>

      {global && (
        <Section
          title="封面搜索"
          description="在工作区「书籍信息」中搜索网络图片作为封面（仅桌面版）。"
        >
          <Field
            label="默认搜索内容"
            hint="{title} 和 {author} 会替换为书名和作者，搜索前仍可修改。"
          >
            <Input
              value={form.cover_search.query}
              onChange={(event) => search({ query: event.target.value })}
              autoComplete="off"
              className="h-9"
            />
          </Field>
          <fieldset className="space-y-2">
            <legend className="mb-2 text-xs font-medium text-muted-foreground">
              图片源：网址中的 {'{query}'} 会替换为搜索内容
            </legend>
            {form.cover_search.engines.map((item, index) => (
              <div key={index} className="flex flex-wrap items-center gap-2">
                <Input
                  aria-label={`图片源 ${index + 1} 名称`}
                  value={item.name}
                  onChange={(event) =>
                    engine(index, { name: event.target.value })
                  }
                  autoComplete="off"
                  className="h-9 w-32"
                />
                <Input
                  aria-label={`图片源 ${index + 1} 网址`}
                  value={item.url}
                  onChange={(event) =>
                    engine(index, { url: event.target.value })
                  }
                  autoComplete="off"
                  spellCheck={false}
                  className="h-9 min-w-[16rem] flex-1 font-mono text-xs"
                />
                <Button
                  type="button"
                  size="icon-sm"
                  variant="ghost"
                  aria-label={`删除图片源 ${item.name || index + 1}`}
                  title="删除"
                  onClick={() => engine(index, null)}
                >
                  <DeleteIcon aria-hidden="true" />
                </Button>
              </div>
            ))}
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() =>
                search({
                  engines: [
                    ...form.cover_search.engines,
                    { name: '', url: 'https://' }
                  ]
                })
              }
            >
              <AddIcon aria-hidden="true" />
              添加图片源
            </Button>
          </fieldset>
        </Section>
      )}

      <Section
        title="模板"
        description="未自定义的项使用内置模板；包文件与目录条目不可修改。"
      >
        <div className="space-y-4">
          {templates.map((item) => {
            const value = form.render.templates[item.key]
            return (
              <div key={item.key} className="space-y-2">
                <label className="flex items-center gap-3">
                  <input
                    type="checkbox"
                    checked={value !== null}
                    disabled={value === null && !builtin}
                    onChange={(event) =>
                      template(
                        item.key,
                        event.target.checked
                          ? (builtin?.[item.key] ?? '')
                          : null
                      )
                    }
                    className="size-4 accent-[var(--md-sys-color-primary)]"
                  />
                  <span className="text-sm font-medium">
                    自定义{item.label}
                  </span>
                </label>
                <p className="text-xs text-muted-foreground">{item.hint}</p>
                {value !== null && (
                  <textarea
                    aria-label={item.label}
                    value={value}
                    onChange={(event) => template(item.key, event.target.value)}
                    spellCheck={false}
                    rows={item.key === 'stylesheet' ? 12 : 5}
                    className="w-full resize-y rounded-xl border border-input bg-background px-3 py-2 font-mono text-xs leading-relaxed"
                  />
                )}
              </div>
            )
          })}
        </div>
      </Section>
    </div>
  )
}

function Section({
  title,
  description,
  children
}: {
  title: string
  description: string
  children: ReactNode
}) {
  return (
    <section className="space-y-4 rounded-[1.75rem] bg-card p-5 sm:p-6">
      <div className="space-y-1">
        <h2 className="text-base font-semibold">{title}</h2>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      {children}
    </section>
  )
}
