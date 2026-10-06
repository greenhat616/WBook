// @vitest-environment jsdom

import type { ReactNode } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Settings } from '../../src/bindings'

const { commands } = vi.hoisted(() => ({
  commands: {
    getSettings: vi.fn(),
    saveSettings: vi.fn(),
    defaultSettings: vi.fn(),
    builtinTemplates: vi.fn()
  }
}))

vi.mock('../../src/transport', () => ({
  invoke: (method: string, params: Record<string, unknown> = {}) =>
    commands[
      method.replace(/_(\w)/g, (_, c: string) =>
        c.toUpperCase()
      ) as keyof typeof commands
    ](...Object.values(params))
}))

import { SettingsPage } from '../../src/pages/settings-page'

const defaults: Settings = {
  toc: {
    mode: 'VBook',
    chapter_marks: ['章', '回', '节', '集'],
    volume_marks: ['部', '卷'],
    max_title_len: 25,
    volume_split: 'Titles',
    chapters_per_volume: 50,
    parts: 10
  },
  filters: [],
  render: {
    layout: 'SingleHtml',
    language: 'zh-CN',
    templates: {
      stylesheet: null,
      document: null,
      section: null,
      paragraph: null
    }
  }
}
const builtin = {
  stylesheet: 'p { text-indent: 2em; }',
  document: '<html>',
  section: '<section>',
  paragraph: '<p>{{ text }}</p>'
}

function renderPage() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  })
  const Wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
  render(<SettingsPage />, { wrapper: Wrapper })
}

beforeEach(() => {
  vi.resetAllMocks()
  commands.builtinTemplates.mockResolvedValue(builtin)
  commands.defaultSettings.mockResolvedValue(defaults)
  commands.saveSettings.mockImplementation(async (settings: Settings) => ({
    settings,
    problem: null
  }))
})

afterEach(cleanup)

describe('settings page', () => {
  it('saves edited parser, filter and template settings', async () => {
    commands.getSettings.mockResolvedValue({
      settings: defaults,
      problem: null
    })
    renderPage()

    const save = await screen.findByRole('button', { name: '保存' })
    expect(save).toHaveProperty('disabled', true)
    fireEvent.click(screen.getByRole('radio', { name: '仅章节' }))
    fireEvent.click(screen.getByRole('checkbox', { name: /清除广告行/ }))
    const stylesheet = screen.getByRole('checkbox', {
      name: '自定义样式表 book.css'
    })
    await waitFor(() => expect(stylesheet).toHaveProperty('disabled', false))
    fireEvent.click(stylesheet)
    expect(
      (
        screen.getByRole('textbox', {
          name: '样式表 book.css'
        }) as HTMLTextAreaElement
      ).value
    ).toBe(builtin.stylesheet)

    fireEvent.click(save)
    await screen.findByText('设置已保存')
    expect(commands.saveSettings).toHaveBeenCalledWith({
      ...defaults,
      toc: { ...defaults.toc, mode: 'Chapters' },
      filters: ['Ad'],
      render: {
        ...defaults.render,
        templates: {
          ...defaults.render.templates,
          stylesheet: builtin.stylesheet
        }
      }
    })
    expect(save).toHaveProperty('disabled', true)
  })

  it('reports an unusable settings file and restores defaults without saving', async () => {
    commands.getSettings.mockResolvedValue({
      settings: { ...defaults, filters: ['Ad'] },
      problem: 'settings.toml is not valid TOML'
    })
    renderPage()

    expect((await screen.findByRole('alert')).textContent).toContain(
      'settings.toml is not valid TOML'
    )
    fireEvent.click(screen.getByRole('button', { name: '恢复默认' }))
    await screen.findByText('已载入恢复默认，保存后生效')
    expect(screen.getByRole('checkbox', { name: /清除广告行/ })).toHaveProperty(
      'checked',
      false
    )
    expect(commands.saveSettings).not.toHaveBeenCalled()
  })

  it('blocks saving invalid parser settings and shows backend rejections', async () => {
    commands.getSettings.mockResolvedValue({
      settings: defaults,
      problem: null
    })
    commands.saveSettings.mockRejectedValue({
      kind: 'invalid_config',
      message: 'invalid language'
    })
    renderPage()

    const marks = (await screen.findByDisplayValue(
      '章 回 节 集'
    )) as HTMLInputElement
    fireEvent.change(marks, { target: { value: ' ' } })
    expect(screen.getByText('至少需要一个章节标记')).toBeTruthy()
    expect(screen.getByRole('button', { name: '保存' })).toHaveProperty(
      'disabled',
      true
    )

    fireEvent.change(marks, { target: { value: '话' } })
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    expect((await screen.findByText(/invalid language/)).textContent).toContain(
      'invalid_config'
    )
  })
})
