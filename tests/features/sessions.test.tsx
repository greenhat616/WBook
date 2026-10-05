// @vitest-environment jsdom

import { StrictMode } from 'react'
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
  Outcome,
  PreviewInfo,
  SessionSnapshot,
  WorkspaceResults
} from '../../src/bindings'

const { commands, subscribe } = vi.hoisted(() => ({
  commands: {
    listSessions: vi.fn(),
    createSession: vi.fn(),
    getSession: vi.fn(),
    readResults: vi.fn(),
    initializeSession: vi.fn(),
    renderPreview: vi.fn(),
    exportEpub: vi.fn(),
    cancelOperation: vi.fn(),
    closeSession: vi.fn()
  },
  subscribe: vi.fn()
}))

vi.mock('../../src/bindings', () => ({ commands }))
vi.mock('../../src/bridge', () => ({ subscribeSession: subscribe }))

import { exportOptions } from '../../src/features/sessions/api'
import { useSession } from '../../src/features/sessions/use-session'
import { useSessions } from '../../src/features/sessions/use-sessions'
import { SessionPage } from '../../src/pages/session-page'

const ok = <T,>(data: T): Outcome<T> => ({ status: 'ok', data })
const receipt = <T,>(data: T, revision = 1): Outcome<OperationResponse<T>> =>
  ok({
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
  commands.getSession.mockImplementation(async () => ok(backend))
  commands.listSessions.mockResolvedValue(ok([]))
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
    const listing = deferred<Outcome<SessionSnapshot[]>>()
    commands.listSessions.mockReturnValue(listing.promise)
    commands.createSession.mockResolvedValue(ok(snapshot()))
    const { result } = renderHook(useSessions)
    await act(async () => {
      await result.current.create(' C:/book.txt ', 4)
    })
    expect(commands.createSession).toHaveBeenCalledWith('C:/book.txt', {
      filters: [],
      toc: { SplitEvenly: { parts: 4 } }
    })
    await act(async () => {
      listing.resolve(ok([]))
    })
    expect(result.current.sessions).toHaveLength(1)
    expect(result.current.loading).toBe(false)
  })

  it('validates creation and reports command failures without throwing', async () => {
    const { result } = renderHook(useSessions)
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => {
      expect(await result.current.create('book.txt', 0)).toBeNull()
    })
    expect(commands.createSession).not.toHaveBeenCalled()
    commands.createSession.mockResolvedValue({
      status: 'error',
      error: { kind: 'invalid_config', message: 'Invalid rules' }
    })
    await act(async () => {
      expect(await result.current.create('book.txt', 1)).toBeNull()
    })
    expect(result.current.error).toContain('invalid_config')
  })
})

describe('active session', () => {
  it('does not write on StrictMode mount and releases old subscriptions and responses', async () => {
    const old = deferred<Outcome<SessionSnapshot>>()
    commands.getSession.mockImplementation((id: number) =>
      id === 1 ? old.promise : Promise.resolve(ok(snapshot(2)))
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
      old.resolve(ok(snapshot(1, 99)))
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
    commands.initializeSession.mockResolvedValue(
      ok({
        op: 1,
        kind: 'Initialize',
        revision: 0,
        warnings: [warning],
        outcome: {
          status: 'error',
          error: { kind: 'extractor', message: 'Missing input' }
        }
      })
    )
    await act(async () => {
      await result.current.initialize()
    })
    expect(result.current.warnings).toEqual([warning])
    expect(result.current.error).toContain('操作失败 (extractor)')
    commands.initializeSession.mockResolvedValue({
      status: 'error',
      error: { kind: 'busy', message: 'Busy' }
    })
    await act(async () => {
      await result.current.initialize()
    })
    expect(result.current.error).toContain('命令失败 (busy)')
    expect(result.current.warnings).toEqual([warning])
  })

  it('serializes mutations and refresh while cancellation remains available', async () => {
    const initialization = deferred<Outcome<OperationResponse<number>>>()
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    commands.initializeSession.mockReturnValue(initialization.promise)
    commands.cancelOperation.mockResolvedValue(ok('Requested'))
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
      initialization.resolve(
        ok({
          op: 7,
          kind: 'Initialize',
          revision: 0,
          warnings: [],
          outcome: {
            status: 'error',
            error: { kind: 'cancelled', message: 'Cancelled' }
          }
        })
      )
      await operation
    })
    expect(result.current.error).toContain('cancelled')
    expect(result.current.pending).toBe(false)
  })

  it('closes while an operation is pending and ignores its late success', async () => {
    const initialization = deferred<Outcome<OperationResponse<number>>>()
    const { result } = renderHook(() => useSession(1))
    await waitFor(() => expect(result.current.loading).toBe(false))
    commands.initializeSession.mockReturnValue(initialization.promise)
    commands.closeSession.mockResolvedValue(
      ok({ last: null, lost: false, cleanup_failures: [warning] })
    )
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

    const closing = deferred<Outcome<ClosedSession>>()
    commands.closeSession.mockReturnValue(closing.promise)
    let completion!: Promise<ClosedSession | null>
    act(() => {
      completion = result.current.close()
    })
    unmount()
    closing.resolve(
      ok({ last: null, lost: false, cleanup_failures: [warning] })
    )
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
    const rendering = deferred<Outcome<OperationResponse<PreviewInfo>>>()
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
      .mockResolvedValueOnce(ok(backend))
      .mockResolvedValueOnce(ok(closed))
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

describe('session page close feedback', () => {
  beforeEach(() => {
    // jsdom cannot scroll; these tests exercise navigation and visible feedback.
    vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
  })
  afterEach(() => vi.restoreAllMocks())

  async function openPage() {
    backend = snapshot(1, 1, 1)
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
    commands.closeSession.mockResolvedValue(
      ok({
        last: null,
        lost: false,
        cleanup_failures: [warning]
      })
    )
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
    expect(
      screen.queryByText('可以检查阅读预览，也可以直接导出 EPUB。')
    ).toBeNull()
    expect(screen.getByText('本次会话已关闭，预览已失效。')).toBeTruthy()

    fireEvent.click(screen.getByRole('link', { name: '返回工作台' }))
    await screen.findByRole('heading', { name: '工作台首页' })
    expect(router.state.location.pathname).toBe('/')
  })

  it('returns to the workbench automatically after closing without cleanup warnings', async () => {
    const router = await openPage()
    commands.closeSession.mockResolvedValue(
      ok({
        last: null,
        lost: false,
        cleanup_failures: []
      })
    )
    fireEvent.click(screen.getByRole('button', { name: '关闭会话' }))

    await screen.findByRole('heading', { name: '工作台首页' })
    expect(router.state.location.pathname).toBe('/')
    expect(commands.closeSession).toHaveBeenCalledWith(1)
  })
})
