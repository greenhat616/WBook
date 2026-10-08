// @vitest-environment jsdom

import type { ReactNode } from 'react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider
} from '@tanstack/react-router'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Settings, StoredSettings } from '../../src/bindings'

const { commands } = vi.hoisted(() => ({
  commands: {
    getSettings: vi.fn(),
    saveSettings: vi.fn(),
    defaultSettings: vi.fn(),
    builtinTemplates: vi.fn()
  }
}))

const updates = vi.hoisted(() => ({
  send: null as ((stored: StoredSettings) => void) | null,
  fail: null as ((error: Error) => void) | null
}))

vi.mock('../../src/bridge', () => ({
  subscribeSettings: vi.fn(
    (send: (stored: StoredSettings) => void, fail: (error: Error) => void) => {
      updates.send = send
      updates.fail = fail
      return Promise.resolve(() => {})
    }
  )
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

function renderPage(initialEntries = ['/settings']) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  })
  const Wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
  const root = createRootRoute()
  const router = createRouter({
    routeTree: root.addChildren([
      createRoute({
        getParentRoute: () => root,
        path: '/',
        component: () => <h1>工作台</h1>
      }),
      createRoute({
        getParentRoute: () => root,
        path: '/sessions/$sessionId',
        component: () => <h1>工作区页面</h1>
      }),
      createRoute({
        getParentRoute: () => root,
        path: '/settings',
        component: SettingsPage
      })
    ]),
    history: createMemoryHistory({
      initialEntries,
      initialIndex: initialEntries.length - 1
    })
  })
  render(<RouterProvider router={router} />, { wrapper: Wrapper })
  return router
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
  commands.builtinTemplates.mockResolvedValue(builtin)
  commands.defaultSettings.mockResolvedValue(defaults)
  commands.saveSettings.mockImplementation(
    async (expected: number, settings: Settings) => ({
      settings,
      revision: expected + 1,
      problem: null
    })
  )
  commands.getSettings.mockResolvedValue({
    settings: defaults,
    revision: 0,
    problem: null
  })
  updates.send = null
  updates.fail = null
})

afterEach(() => {
  cleanup()
  vi.unstubAllGlobals()
})

describe('settings page', () => {
  it('closes back to the page it was opened from', async () => {
    const router = renderPage(['/sessions/1', '/settings'])
    fireEvent.click(await screen.findByRole('button', { name: '关闭设置' }))
    await screen.findByRole('heading', { name: '工作区页面' })
    expect(router.state.location.pathname).toBe('/sessions/1')
  })

  it('leaves closing to the window on the desktop', async () => {
    vi.stubGlobal('isTauri', true)
    renderPage()
    await screen.findByRole('button', { name: '保存' })
    expect(screen.queryByRole('button', { name: '关闭设置' })).toBeNull()
  })

  it('closes to the home page when there is nothing to go back to', async () => {
    const router = renderPage()
    fireEvent.click(await screen.findByRole('button', { name: '关闭设置' }))
    await screen.findByRole('heading', { name: '工作台' })
    expect(router.state.location.pathname).toBe('/')
  })

  it('saves edited parser, filter and template settings', async () => {
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
    expect(commands.saveSettings).toHaveBeenCalledWith(0, {
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
      revision: 0,
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

  it('follows saves from other windows while the form is unchanged', async () => {
    renderPage()
    await screen.findByRole('button', { name: '保存' })
    await waitFor(() => expect(updates.send).not.toBeNull())
    act(() =>
      updates.send!({
        settings: { ...defaults, filters: ['Ad'] },
        revision: 1,
        problem: null
      })
    )
    await waitFor(() =>
      expect(
        screen.getByRole('checkbox', { name: /清除广告行/ })
      ).toHaveProperty('checked', true)
    )
    expect(screen.queryByText(/已在其他窗口更新/)).toBeNull()

    fireEvent.click(screen.getByRole('radio', { name: '仅章节' }))
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    await screen.findByText('设置已保存')
    expect(commands.saveSettings).toHaveBeenCalledWith(1, {
      ...defaults,
      toc: { ...defaults.toc, mode: 'Chapters' },
      filters: ['Ad']
    })
  })

  it('keeps edits when another window saves and offers the latest values', async () => {
    renderPage()
    const save = await screen.findByRole('button', { name: '保存' })
    await waitFor(() => expect(updates.send).not.toBeNull())
    fireEvent.click(screen.getByRole('radio', { name: '仅章节' }))
    act(() =>
      updates.send!({
        settings: { ...defaults, filters: ['Ad'] },
        revision: 1,
        problem: null
      })
    )

    expect(
      (await screen.findByText(/已在其他窗口更新/)).getAttribute('role')
    ).toBe('alert')
    expect(
      screen.getByRole('radio', { name: '仅章节' }).getAttribute('aria-checked')
    ).toBe('true')
    expect(save).toHaveProperty('disabled', true)

    fireEvent.click(screen.getByRole('button', { name: '载入最新' }))
    expect(screen.getByRole('checkbox', { name: /清除广告行/ })).toHaveProperty(
      'checked',
      true
    )
    expect(
      screen.getByRole('radio', { name: '仅章节' }).getAttribute('aria-checked')
    ).toBe('false')
    expect(screen.queryByText(/已在其他窗口更新/)).toBeNull()
  })

  it('does not report its own save as a change from another window', async () => {
    renderPage()
    await screen.findByRole('button', { name: '保存' })
    await waitFor(() => expect(updates.send).not.toBeNull())
    fireEvent.click(screen.getByRole('radio', { name: '仅章节' }))
    const saved = { ...defaults, toc: { ...defaults.toc, mode: 'Chapters' } }
    let finish!: (stored: StoredSettings) => void
    commands.saveSettings.mockReturnValue(
      new Promise((resolve) => (finish = resolve))
    )
    fireEvent.click(screen.getByRole('button', { name: '保存' }))
    // The stream announces the save before its response arrives.
    act(() => updates.send!({ settings: saved, revision: 1, problem: null }))
    await act(async () =>
      finish({ settings: saved, revision: 1, problem: null })
    )
    await screen.findByText('设置已保存')
    expect(screen.queryByText(/已在其他窗口更新/)).toBeNull()
  })

  it('reports when updates from other windows stop and reconnects', async () => {
    const { subscribeSettings } = await import('../../src/bridge')
    renderPage()
    await waitFor(() => expect(updates.fail).not.toBeNull())
    act(() => updates.fail!(new Error('Settings subscription disconnected')))
    expect(
      (await screen.findByText(/无法接收其他窗口的设置更新/)).textContent
    ).toContain('disconnected')
    fireEvent.click(screen.getByRole('button', { name: '重新连接' }))
    await waitFor(() => expect(subscribeSettings).toHaveBeenCalledTimes(2))
    expect(screen.queryByText(/无法接收其他窗口的设置更新/)).toBeNull()
  })
})
