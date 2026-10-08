// @vitest-environment jsdom

import { StrictMode, type ReactNode } from 'react'
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
  renderHook,
  screen,
  waitFor
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  OperationResponse,
  ClosedSession,
  ExportOptions,
  Outcome,
  PreviewInfo,
  SessionSnapshot,
  Settings,
  TocEntry_Serialize,
  WorkspaceResults
} from '../../src/bindings'

const { commands, subscribe, isTauri, openDialog, webview } = vi.hoisted(() => {
  // jsdom lacks custom state sets, which the M3E elements toggle on connect.
  if (!('states' in ElementInternals.prototype)) {
    const states = new WeakMap<ElementInternals, Set<string>>()
    Object.defineProperty(ElementInternals.prototype, 'states', {
      get(this: ElementInternals) {
        if (!states.has(this)) states.set(this, new Set())
        return states.get(this)
      }
    })
  }
  return {
    commands: {
      listSessions: vi.fn(),
      openSessionWindow: vi.fn(),
      createSession: vi.fn(),
      getSession: vi.fn(),
      readResults: vi.fn(),
      initializeSession: vi.fn(),
      renderPreview: vi.fn(),
      parseSession: vi.fn(),
      getSessionSettings: vi.fn(),
      setSessionSettings: vi.fn(),
      getSettings: vi.fn(),
      builtinTemplates: vi.fn(),
      installResults: vi.fn(),
      setMetadataOverrides: vi.fn(),
      readText: vi.fn(),
      exportEpub: vi.fn(),
      cancelOperation: vi.fn(),
      closeSession: vi.fn()
    },
    subscribe: vi.fn(),
    isTauri: vi.fn(),
    openDialog: vi.fn(),
    webview: {
      drop: null as null | ((event: { payload: unknown }) => void),
      events: new Map<string, (event: { payload: unknown }) => void>()
    }
  }
})

vi.mock('../../src/transport', () => ({
  invoke: (method: string, params: Record<string, unknown> = {}) =>
    commands[
      method.replace(/_(\w)/g, (_, c: string) =>
        c.toUpperCase()
      ) as keyof typeof commands
    ](...Object.values(params))
}))
vi.mock('../../src/bridge', () => ({ subscribeSession: subscribe }))
vi.mock('@tauri-apps/api/core', () => ({ isTauri }))
vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openDialog,
  save: vi.fn()
}))
vi.mock('@tauri-apps/api/webview', () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (event: { payload: unknown }) => void) => {
      webview.drop = handler
      return Promise.resolve(() => {
        webview.drop = null
      })
    },
    listen: (name: string, handler: (event: { payload: unknown }) => void) => {
      webview.events.set(name, handler)
      return Promise.resolve(() => webview.events.delete(name))
    }
  })
}))

import { useSession } from '../../src/features/sessions/use-session'
import { useSessions } from '../../src/features/sessions/use-sessions'
import { HomePage } from '../../src/pages/home-page'
import { SessionPage } from '../../src/pages/session-page'
import { SessionSettingsPage } from '../../src/pages/session-settings-page'

const settings: Settings = {
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
const exportOptions: ExportOptions = {
  render: {
    layout: settings.render.layout,
    templates: settings.render.templates
  },
  format: 'Epub',
  language: settings.render.language,
  identifier: null
}
const ok = <T,>(data: T): Outcome<T> => ({ status: 'ok', data })
const receipt = <T,>(data: T, revision = 1): OperationResponse<T> => ({
  op: 1,
  kind: 'Initialize',
  revision,
  outcome: ok(data),
  warnings: []
})
const snapshot = (session = 1, seq = 0, revision = 0): SessionSnapshot => ({
  session,
  seq,
  workspace: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  source: `C:/book-${session}.txt`,
  lifecycle: 'Open',
  activity: 'Idle',
  last: null,
  workspace_status: {
    Available: {
      revision,
      document: revision ? 'Current' : 'Absent',
      document_version: null,
      filters: { applied: 0, total: 0 },
      has_overrides: false,
      preview: null,
      preview_id: null
    }
  }
})
const results: WorkspaceResults = {
  results: null,
  current: true,
  overrides: { title: null, author: null }
}
const preview: PreviewInfo = {
  id: 'preview-1',
  revision: 1,
  options: exportOptions,
  directory: 'C:/temp/preview-1',
  files: ['book.xhtml']
}
const warning = { path: 'C:/temp/old-preview', message: 'File is still in use' }

function queryWrapper() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } }
  })
  return function QueryWrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((complete) => {
    resolve = complete
  })
  return { promise, resolve }
}

let backend: SessionSnapshot
let subscriptions: Array<{
  session: number
  receive: (snapshot: SessionSnapshot) => void
  error: (error: Error) => void
  signal: AbortSignal
  stop: ReturnType<typeof vi.fn>
}>

beforeEach(() => {
  vi.resetAllMocks()
  backend = snapshot()
  subscriptions = []
  commands.getSession.mockImplementation(async () => backend)
  commands.listSessions.mockResolvedValue([])
  commands.getSessionSettings.mockResolvedValue(settings)
  commands.readResults.mockImplementation(async () =>
    receipt(results, backend.workspace_status.Available!.revision)
  )
  subscribe.mockImplementation(
    async (session, receive, error, signal: AbortSignal) => {
      const stop = vi.fn()
      signal.addEventListener('abort', stop, { once: true })
      subscriptions.push({ session, receive, error, signal, stop })
      return stop
    }
  )
})

afterEach(cleanup)

describe('session list', () => {
  it('preserves a created session when an older list response arrives later', async () => {
    const listing = deferred<SessionSnapshot[]>()
    commands.listSessions.mockReturnValue(listing.promise)
    commands.createSession.mockResolvedValue(snapshot())
    const { result } = renderHook(useSessions, { wrapper: queryWrapper() })
    await act(async () => {
      await result.current.create(' C:/book.txt ')
    })
    expect(commands.createSession).toHaveBeenCalledWith('C:/book.txt')
    await act(async () => {
      listing.resolve([])
    })
    expect(result.current.sessions).toHaveLength(1)
    expect(result.current.loading).toBe(false)
  })

  it('validates creation and reports command failures without throwing', async () => {
    const { result } = renderHook(useSessions, { wrapper: queryWrapper() })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => {
      expect(await result.current.create('  ')).toBeNull()
    })
    expect(commands.createSession).not.toHaveBeenCalled()
    commands.createSession.mockRejectedValue({
      kind: 'invalid_config',
      message: 'Invalid rules'
    })
    await act(async () => {
      expect(await result.current.create('book.txt')).toBeNull()
    })
    expect(result.current.error).toContain('invalid_config')
  })
})

describe('active session', () => {
  it('does not write on StrictMode mount and releases old subscriptions and responses', async () => {
    const old = deferred<SessionSnapshot>()
    commands.getSession.mockImplementation((id: number) =>
      id === 1 ? old.promise : Promise.resolve(snapshot(2))
    )
    const { result, rerender, unmount } = renderHook(
      ({ id }) => useSession(id),
      {
        initialProps: { id: 1 },
        wrapper: StrictMode
      }
    )
    rerender({ id: 2 })
    await waitFor(() => expect(result.current.snapshot?.session).toBe(2))
    await act(async () => {
      old.resolve(snapshot(1, 99))
      subscriptions
        .find((item) => item.session === 1)!
        .receive(snapshot(1, 100))
    })
    expect(result.current.snapshot?.session).toBe(2)
    expect(commands.initializeSession).not.toHaveBeenCalled()
    expect(commands.createSession).not.toHaveBeenCalled()
    expect(
      subscriptions
        .filter((item) => item.session === 1)
        .every((item) => item.signal.aborted)
    ).toBe(true)
    unmount()
    expect(subscriptions.every((item) => item.signal.aborted)).toBe(true)
  })

  it('keeps warnings on failed outcomes and distinguishes command errors', async () => {
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    commands.initializeSession.mockResolvedValue({
      op: 1,
      kind: 'Initialize',
      revision: 0,
      warnings: [warning],
      outcome: {
        status: 'error',
        error: { kind: 'extractor', message: 'Missing input' }
      }
    })
    await act(async () => {
      await result.current.initialize()
    })
    expect(result.current.warnings).toEqual([warning])
    expect(result.current.error).toContain('操作失败 (extractor)')
    commands.initializeSession.mockRejectedValue({
      kind: 'busy',
      message: 'Busy'
    })
    await act(async () => {
      await result.current.initialize()
    })
    expect(result.current.error).toContain('命令失败 (busy)')
    expect(result.current.warnings).toEqual([warning])
  })

  it('serializes mutations and refresh while cancellation remains available', async () => {
    const initialization = deferred<OperationResponse<number>>()
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    commands.initializeSession.mockReturnValue(initialization.promise)
    commands.cancelOperation.mockResolvedValue('Requested')
    let operation!: Promise<void>
    act(() => {
      operation = result.current.initialize()
    })
    await act(async () => {
      await result.current.initialize()
      await result.current.refresh()
    })
    expect(commands.initializeSession).toHaveBeenCalledTimes(1)
    backend = {
      ...snapshot(1, 1),
      activity: {
        Running: {
          op: 7,
          kind: 'Initialize',
          phase: 'Extracting',
          cancel_requested: false
        }
      }
    }
    await act(async () => {
      await result.current.cancel()
    })
    expect(commands.cancelOperation).toHaveBeenCalledWith(1, 7)
    expect(result.current.notice).toContain('已请求取消')
    expect(result.current.notice).not.toContain('完成')
    await act(async () => {
      initialization.resolve({
        op: 7,
        kind: 'Initialize',
        revision: 0,
        warnings: [],
        outcome: {
          status: 'error',
          error: { kind: 'cancelled', message: 'Cancelled' }
        }
      })
      await operation
    })
    expect(result.current.error).toContain('cancelled')
    expect(result.current.pending).toBe(false)
  })

  it('closes while an operation is pending and ignores its late success', async () => {
    const initialization = deferred<OperationResponse<number>>()
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    commands.initializeSession.mockReturnValue(initialization.promise)
    commands.closeSession.mockResolvedValue({
      last: null,
      lost: false,
      cleanup_failures: [warning]
    })
    let operation!: Promise<void>
    act(() => {
      operation = result.current.initialize()
    })
    await act(async () => {
      expect(await result.current.close()).toEqual({
        last: null,
        lost: false,
        cleanup_failures: [warning]
      })
    })
    await act(async () => {
      initialization.resolve(receipt(1))
      await operation
    })
    expect(result.current.connection).toBe('closed')
    expect(result.current.notice).toBe('会话已关闭')
    expect(result.current.preview).toBeNull()
    expect(result.current.warnings).toEqual([warning])
    expect(commands.readResults).not.toHaveBeenCalled()
    expect(subscriptions[0].signal.aborted).toBe(true)
  })

  it('returns no close report on failure or after unmount', async () => {
    const { result, unmount } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    commands.closeSession.mockRejectedValueOnce(new Error('Close failed'))
    await act(async () => {
      expect(await result.current.close()).toBeNull()
    })
    expect(result.current.error).toContain('Close failed')

    const closing = deferred<ClosedSession>()
    commands.closeSession.mockReturnValue(closing.promise)
    let completion!: Promise<ClosedSession | null>
    act(() => {
      completion = result.current.close()
    })
    unmount()
    closing.resolve({ last: null, lost: false, cleanup_failures: [warning] })
    expect(await completion).toBeNull()
  })

  it('invalidates stale results and previews on newer snapshots without a read loop', async () => {
    backend = snapshot(1, 1, 1)
    commands.renderPreview.mockImplementation(async () => {
      backend = snapshot(1, 3, 1)
      backend.workspace_status.Available!.preview = exportOptions
      backend.workspace_status.Available!.preview_id = preview.id
      return receipt(preview)
    })
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => {
      await result.current.renderPreview()
    })
    expect(result.current.preview).toEqual(preview)
    expect(result.current.results).toEqual(results)
    const reads = commands.readResults.mock.calls.length
    act(() => {
      subscriptions[0].receive(snapshot(1, 2, 0))
      subscriptions[0].receive(snapshot(1, 4, 2))
    })
    expect(result.current.snapshot?.seq).toBe(4)
    expect(result.current.preview).toBeNull()
    expect(result.current.results).toBeNull()
    expect(commands.readResults).toHaveBeenCalledTimes(reads)
  })

  it('invalidates a preview when coalesced snapshots replace its ID at the same revision and options', async () => {
    backend = snapshot(1, 1, 1)
    commands.renderPreview.mockImplementation(async () => {
      backend = snapshot(1, 3, 1)
      backend.workspace_status.Available!.preview = exportOptions
      backend.workspace_status.Available!.preview_id = preview.id
      return receipt(preview)
    })
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => {
      await result.current.renderPreview()
    })
    expect(result.current.preview).toEqual(preview)
    const reads = commands.readResults.mock.calls.length

    const replacement = snapshot(1, 9, preview.revision)
    replacement.workspace_status.Available!.preview = preview.options
    replacement.workspace_status.Available!.preview_id = 'preview-3'
    act(() => {
      subscriptions[0].receive(replacement)
    })

    expect(result.current.snapshot).toEqual(replacement)
    expect(result.current.preview).toBeNull()
    expect(result.current.results).toEqual(results)
    expect(commands.readResults).toHaveBeenCalledTimes(reads)
  })

  it('does not revive a replaced preview when its render receipt and refresh arrive late', async () => {
    backend = snapshot(1, 1, 1)
    const rendering = deferred<OperationResponse<PreviewInfo>>()
    commands.renderPreview.mockReturnValue(rendering.promise)
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    let operation!: Promise<void>
    act(() => {
      operation = result.current.renderPreview()
    })

    const replacement = snapshot(1, 9, preview.revision)
    replacement.workspace_status.Available!.preview = preview.options
    replacement.workspace_status.Available!.preview_id = 'preview-3'
    act(() => {
      subscriptions[0].receive(replacement)
    })
    backend = snapshot(1, 3, preview.revision)
    backend.workspace_status.Available!.preview = preview.options
    backend.workspace_status.Available!.preview_id = preview.id
    await act(async () => {
      rendering.resolve(receipt(preview))
      await operation
    })

    expect(result.current.snapshot).toEqual(replacement)
    expect(result.current.preview).toBeNull()
    expect(result.current.notice).not.toBe('预览已生成')
    expect(result.current.pending).toBe(false)
  })

  it('does not restore read results after the confirming snapshot closes the session', async () => {
    backend = snapshot(1, 1, 1)
    const closed = { ...snapshot(1, 3, 1), lifecycle: 'Closed' as const }
    commands.getSession
      .mockResolvedValueOnce(backend)
      .mockResolvedValueOnce(closed)
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))

    expect(commands.readResults).toHaveBeenCalledTimes(1)
    expect(result.current.snapshot).toEqual(closed)
    expect(result.current.connection).toBe('closed')
    expect(result.current.results).toBeNull()
    expect(result.current.preview).toBeNull()
  })

  it('reports disconnection and only reconnects when explicitly requested', async () => {
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    act(() => {
      subscriptions[0].error(new Error('Disconnected'))
    })
    expect(result.current.connection).toBe('disconnected')
    expect(subscribe).toHaveBeenCalledTimes(1)
    act(() => {
      result.current.reconnect()
    })
    expect(subscriptions[0].signal.aborted).toBe(true)
    expect(subscribe).toHaveBeenCalledTimes(2)
    act(() => {
      subscriptions[1].receive(snapshot())
    })
    expect(result.current.connection).toBe('live')
  })

  it('preserves the actual export path even when its subsequent refresh fails', async () => {
    backend = snapshot(1, 1, 1)
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    commands.exportEpub.mockResolvedValue(
      receipt({
        path: 'C:/output/book.epub',
        revision: 1,
        cleanup_failures: [warning]
      })
    )
    commands.getSession.mockRejectedValue(
      new Error('Connection lost during refresh')
    )
    await act(async () => {
      await result.current.exportBook('C:/output/book.epub')
    })
    expect(result.current.exportPath).toBe('C:/output/book.epub')
    expect(result.current.notice).toBe('EPUB 已导出')
    expect(result.current.warnings).toEqual([warning])
    expect(result.current.error).toContain('Connection lost')
  })
})

describe('session page', () => {
  const version = { document_id: Array(16).fill(0), revision: 3 }
  const entry = (
    id: number,
    title: string,
    start: number,
    children: TocEntry_Serialize[] = []
  ): TocEntry_Serialize => ({
    id,
    title,
    meta: { words: 0, range_kind: 'Heading', range: { start, end: start + 9 } },
    children
  })
  const parsed = (toc: TocEntry_Serialize[]): WorkspaceResults => ({
    results: {
      version: { ...version, document_id: [...version.document_id] },
      toc,
      metadata: { title: '原书名', author: '作者甲' }
    },
    current: true,
    overrides: { title: null, author: null }
  })
  const book = parsed([
    entry(1, '第一卷', 0, [
      entry(2, '第1章 开始', 10),
      entry(3, '第2章 继续', 2058)
    ])
  ])

  beforeEach(() => {
    // jsdom cannot scroll; these tests exercise navigation and visible feedback.
    vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
  })
  afterEach(() => vi.restoreAllMocks())

  async function openPage(shown: WorkspaceResults = results) {
    backend = snapshot(1, 1, 1)
    backend.workspace_status.Available!.document_version = {
      ...version,
      document_id: [...version.document_id]
    }
    commands.readResults.mockImplementation(async () =>
      receipt(shown, backend.workspace_status.Available!.revision)
    )
    const root = createRootRoute()
    const home = createRoute({
      getParentRoute: () => root,
      path: '/',
      component: () => <h1>工作台首页</h1>
    })
    const session = createRoute({
      getParentRoute: () => root,
      path: '/sessions/$sessionId',
      component: () => <SessionPage sessionId={1} />
    })
    const router = createRouter({
      routeTree: root.addChildren([home, session]),
      history: createMemoryHistory({ initialEntries: ['/sessions/1'] })
    })
    render(<RouterProvider router={router} />)
    await screen.findByRole('heading', { name: 'book-1.txt' })
    await waitFor(() => {
      expect(
        (screen.getByRole('button', { name: '生成预览' }) as HTMLButtonElement)
          .disabled
      ).toBe(false)
    })
    return router
  }

  it('keeps cleanup warnings visible and disables actions before the closed SSE snapshot arrives', async () => {
    const router = await openPage()
    commands.closeSession.mockResolvedValue({
      last: null,
      lost: false,
      cleanup_failures: [warning]
    })
    fireEvent.click(screen.getByRole('button', { name: '关闭会话' }))
    await screen.findByText(warning.message)

    expect(screen.getByText(warning.path)).toBeTruthy()
    expect(router.state.location.pathname).toBe('/sessions/1')
    expect(backend.lifecycle).toBe('Open')
    for (const name of ['关闭会话', '刷新状态', '生成预览', '导出 EPUB']) {
      expect(
        (screen.getByRole('button', { name }) as HTMLButtonElement).disabled
      ).toBe(true)
    }
    fireEvent.click(screen.getByRole('tab', { name: '预览' }))
    expect(screen.getByText('本次会话已关闭，预览已失效。')).toBeTruthy()

    fireEvent.click(screen.getAllByRole('link', { name: '返回工作台' })[0])
    await screen.findByRole('heading', { name: '工作台首页' })
    expect(router.state.location.pathname).toBe('/')
  })

  it('returns to the workbench automatically after closing without cleanup warnings', async () => {
    const router = await openPage()
    commands.closeSession.mockResolvedValue({
      last: null,
      lost: false,
      cleanup_failures: []
    })
    fireEvent.click(screen.getByRole('button', { name: '关闭会话' }))

    await screen.findByRole('heading', { name: '工作台首页' })
    expect(router.state.location.pathname).toBe('/')
    expect(commands.closeSession).toHaveBeenCalledWith(1)
  })

  it('lists chapters with their sizes and reads the selected chapter', async () => {
    commands.readText.mockResolvedValue(receipt('第1章 开始\n正文内容', 1))
    await openPage(book)
    expect(await screen.findByText('1 卷 · 2 章')).toBeTruthy()
    // A chapter runs until the next heading; the last one has no known end.
    expect(screen.getByText('2.0 K')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: /第1章 开始/ }))
    expect(await screen.findByText(/正文内容/)).toBeTruthy()
    expect(commands.readText).toHaveBeenCalledWith(
      1,
      backend.workspace_status.Available!.document_version,
      { start: 10, end: 2058 }
    )

    fireEvent.click(screen.getByRole('button', { name: /第2章 继续/ }))
    expect(await screen.findByText(/无法确定这一章的结束位置/)).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: '收起第一卷' }))
    expect(screen.queryByRole('button', { name: /第1章 开始/ })).toBeNull()
  })

  it('previews a trial parse and installs it only when applied', async () => {
    await openPage(book)
    const reparsed = parsed([entry(5, '第1章 新开始', 10)])
    commands.setSessionSettings.mockResolvedValue(receipt(1, 1))
    commands.parseSession.mockResolvedValue(receipt(reparsed.results, 1))
    commands.installResults.mockResolvedValue(receipt(2, 1))

    fireEvent.click(screen.getByRole('tab', { name: '解析规则' }))
    fireEvent.click(await screen.findByRole('radio', { name: '仅章节' }))
    fireEvent.click(screen.getByRole('button', { name: '试解析' }))
    await screen.findByRole('heading', { name: '试解析目录' })
    // Changed rules become the session's settings before parsing with them.
    expect(commands.setSessionSettings).toHaveBeenCalledWith(1, 1, {
      ...settings,
      toc: { ...settings.toc, mode: 'Chapters' }
    })
    expect(commands.parseSession).toHaveBeenCalledWith(1)
    expect(screen.getByText(/新目录 1 章，当前 1 卷 · 2 章/)).toBeTruthy()
    expect(commands.installResults).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: '放弃' }))
    await screen.findByRole('heading', { name: '目录' })

    fireEvent.click(screen.getByRole('button', { name: '试解析' }))
    fireEvent.click(await screen.findByRole('button', { name: '应用新目录' }))
    await waitFor(() =>
      expect(commands.installResults).toHaveBeenCalledWith(
        1,
        1,
        reparsed.results
      )
    )
  })

  it('stores edited title and author as overrides', async () => {
    await openPage(book)
    commands.setMetadataOverrides.mockResolvedValue(receipt(null, 1))
    const title = (await screen.findByLabelText('书名')) as HTMLInputElement
    await waitFor(() => expect(title.placeholder).toBe('原书名'))
    fireEvent.change(title, { target: { value: ' 新书名 ' } })
    fireEvent.blur(title)
    await waitFor(() =>
      expect(commands.setMetadataOverrides).toHaveBeenCalledWith(1, 1, {
        title: '新书名',
        author: null
      })
    )
  })
})

describe('home page session entry', () => {
  beforeEach(() => {
    vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
    commands.createSession.mockResolvedValue(snapshot())
  })
  afterEach(() => vi.restoreAllMocks())

  function renderHome() {
    const root = createRootRoute()
    const home = createRoute({
      getParentRoute: () => root,
      path: '/',
      component: HomePage
    })
    const session = createRoute({
      getParentRoute: () => root,
      path: '/sessions/$sessionId',
      component: () => <h1>工作区页面</h1>
    })
    const router = createRouter({
      routeTree: root.addChildren([home, session]),
      history: createMemoryHistory({ initialEntries: ['/'] })
    })
    const Wrapper = queryWrapper()
    render(
      <Wrapper>
        <RouterProvider router={router} />
      </Wrapper>
    )
    return router
  }

  function sessionItem(name: RegExp) {
    return screen.getByText(name).closest('m3e-list-action')!
  }

  it('adds picked files and opens them in their own desktop windows', async () => {
    isTauri.mockReturnValue(true)
    openDialog.mockResolvedValue(['C:/book-1.txt'])
    commands.openSessionWindow.mockResolvedValue(null)
    const router = renderHome()
    fireEvent.click(
      (await screen.findByText('选择文件')).closest('m3e-button')!
    )
    await screen.findByText('C:/book-1.txt')
    expect(commands.createSession).toHaveBeenCalledWith('C:/book-1.txt')
    await waitFor(() =>
      expect(commands.openSessionWindow).toHaveBeenCalledWith(1)
    )

    fireEvent.click(sessionItem(/^book-1\.txt$/))
    await waitFor(() =>
      expect(commands.openSessionWindow).toHaveBeenCalledTimes(2)
    )
    expect(router.state.location.pathname).toBe('/')
  })

  it('adds a session for every file dropped on the window', async () => {
    isTauri.mockReturnValue(true)
    commands.createSession.mockImplementation((source: string) =>
      Promise.resolve(snapshot(Number(source.match(/\d+/)![0])))
    )
    renderHome()
    await waitFor(() => expect(webview.drop).not.toBeNull())
    act(() =>
      webview.drop!({ payload: { type: 'enter', paths: ['C:/book-1.txt'] } })
    )
    expect(screen.getByText('松开以添加工作会话')).toBeTruthy()
    act(() =>
      webview.drop!({
        payload: { type: 'drop', paths: ['C:/book-1.txt', 'C:/book-2.txt'] }
      })
    )
    await screen.findByText('C:/book-2.txt')
    expect(screen.getByText('C:/book-1.txt')).toBeTruthy()
    expect(screen.queryByText('松开以添加工作会话')).toBeNull()
  })

  it('drops a session once the host reports its window closed it', async () => {
    isTauri.mockReturnValue(true)
    commands.listSessions.mockResolvedValue([snapshot(1), snapshot(2)])
    renderHome()
    await screen.findByText('C:/book-2.txt')
    await waitFor(() => expect(webview.events.has('session-closed')).toBe(true))

    act(() =>
      webview.events.get('session-closed')!({ payload: { session: 1 } })
    )
    await waitFor(() => expect(screen.queryByText('C:/book-1.txt')).toBeNull())
    expect(screen.getByText('C:/book-2.txt')).toBeTruthy()
  })

  it('reports a window that cannot be opened', async () => {
    isTauri.mockReturnValue(true)
    commands.listSessions.mockResolvedValue([snapshot()])
    commands.openSessionWindow.mockRejectedValue({
      kind: 'closed',
      message: 'Session is closed'
    })
    renderHome()
    await screen.findByText('C:/book-1.txt')
    fireEvent.click(sessionItem(/^book-1\.txt$/))
    expect((await screen.findByRole('alert')).textContent).toContain(
      'Session is closed'
    )
  })

  it('adds typed paths and navigates within the page in the browser', async () => {
    isTauri.mockReturnValue(false)
    const router = renderHome()
    fireEvent.change(await screen.findByLabelText('文本文件路径'), {
      target: { value: 'C:/book-1.txt' }
    })
    fireEvent.click(screen.getByRole('button', { name: '添加' }))
    await screen.findByRole('heading', { name: '工作区页面' })
    expect(router.state.location.pathname).toBe('/sessions/1')
    expect(commands.openSessionWindow).not.toHaveBeenCalled()
  })

  it('explains that browsers cannot add dropped files', async () => {
    isTauri.mockReturnValue(false)
    renderHome()
    await screen.findByRole('heading', { name: '开始一本新书' })
    const files = { types: ['Files'] }
    fireEvent.dragEnter(window, { dataTransfer: files })
    expect(await screen.findByText('松开以添加工作会话')).toBeTruthy()
    fireEvent.drop(window, { dataTransfer: files })
    expect((await screen.findByRole('alert')).textContent).toContain(
      '浏览器无法读取拖入文件的路径'
    )
    expect(commands.createSession).not.toHaveBeenCalled()
  })
})

describe('session settings page', () => {
  beforeEach(() => {
    vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
    commands.builtinTemplates.mockResolvedValue({
      stylesheet: '',
      document: '',
      section: '',
      paragraph: ''
    })
  })
  afterEach(() => vi.restoreAllMocks())

  function openSettings() {
    backend = snapshot(1, 1, 3)
    const root = createRootRoute()
    const page = createRoute({
      getParentRoute: () => root,
      path: '/sessions/$sessionId/settings',
      component: () => <SessionSettingsPage sessionId={1} />
    })
    const workbench = createRoute({
      getParentRoute: () => root,
      path: '/sessions/$sessionId',
      component: () => <h1>工作区</h1>
    })
    const router = createRouter({
      routeTree: root.addChildren([page, workbench]),
      history: createMemoryHistory({
        initialEntries: ['/sessions/1/settings']
      })
    })
    const Wrapper = queryWrapper()
    render(
      <Wrapper>
        <RouterProvider router={router} />
      </Wrapper>
    )
  }

  it('saves against the current revision and can start from the global settings', async () => {
    const global: Settings = {
      ...settings,
      filters: ['Ad'],
      render: { ...settings.render, language: 'zh-Hant' }
    }
    commands.getSettings.mockResolvedValue({ settings: global, problem: null })
    commands.setSessionSettings.mockImplementation(
      async (_id: number, _revision: number, next: Settings) => {
        commands.getSessionSettings.mockResolvedValue(next)
        backend = snapshot(1, 2, 4)
        return receipt(4, 4)
      }
    )
    openSettings()
    await screen.findByRole('heading', { name: '本书设置 · book-1.txt' })
    fireEvent.click(
      await screen.findByRole('button', { name: '恢复为全局设置' })
    )
    await screen.findByText('已载入恢复为全局设置，保存后生效')
    const save = screen.getByRole('button', { name: '保存' })
    fireEvent.click(save)
    await screen.findByText('设置已保存')
    expect(commands.setSessionSettings).toHaveBeenCalledWith(1, 3, global)
    await waitFor(() => expect(save).toHaveProperty('disabled', true))
  })
})
