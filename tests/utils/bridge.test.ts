import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { isTauri, invoke } = vi.hoisted(() => ({
  isTauri: vi.fn(),
  invoke: vi.fn()
}))
vi.mock('@tauri-apps/api/core', () => ({ isTauri, invoke }))

import {
  previewUrl,
  subscribeSession,
  subscribeSettings
} from '../../src/bridge'
import type {
  PreviewInfo,
  SessionSnapshot,
  StoredSettings
} from '../../src/bindings'

class TestEventSource extends EventTarget {
  static instances: TestEventSource[] = []
  close = vi.fn()

  constructor(readonly url: string) {
    super()
    TestEventSource.instances.push(this)
  }

  send(type: string, data: unknown): void {
    this.dispatchEvent(new MessageEvent(type, { data: JSON.stringify(data) }))
  }
}

const snapshot = (seq = 0, lifecycle = 'Open'): SessionSnapshot =>
  ({ session: 1, seq, lifecycle }) as SessionSnapshot
const preview = { id: 'preview-id' } as PreviewInfo
const current = () => TestEventSource.instances.at(-1)!

beforeEach(() => {
  vi.resetAllMocks()
  vi.stubGlobal('EventSource', TestEventSource)
  vi.stubGlobal('window', { location: { href: 'http://localhost:1420/' } })
  vi.stubEnv('VITE_WBOOK_RPC_URL', '')
  TestEventSource.instances = []
  isTauri.mockReturnValue(false)
})

afterEach(() => {
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe('bridge addresses', () => {
  it('resolves browser previews against the RPC origin and encodes segments', async () => {
    vi.stubEnv('VITE_WBOOK_RPC_URL', 'http://127.0.0.1:1421/bridge/rpc')
    expect(await previewUrl(1, preview, 'text/chapter 1.xhtml')).toBe(
      'http://127.0.0.1:1421/bridge/preview/1/preview-id/text/chapter%201.xhtml'
    )
    expect(invoke).not.toHaveBeenCalled()
  })

  it('uses the actual desktop port', async () => {
    isTauri.mockReturnValue(true)
    invoke.mockResolvedValue(52123)
    await subscribeSession(1, vi.fn(), vi.fn())
    expect(invoke).toHaveBeenCalledWith('get_port', {})
    expect(current().url).toBe(
      'http://127.0.0.1:52123/bridge/sessions/1/events'
    )
    expect(await previewUrl(1, preview, 'nav.xhtml')).toBe(
      'http://127.0.0.1:52123/bridge/preview/1/preview-id/nav.xhtml'
    )
  })

  it('rejects invalid identifiers and invalid desktop ports', async () => {
    await expect(subscribeSession(0, vi.fn(), vi.fn())).rejects.toThrow(
      'Invalid session'
    )
    await expect(
      previewUrl(Number.MAX_SAFE_INTEGER + 1, preview, 'nav.xhtml')
    ).rejects.toThrow()
    isTauri.mockReturnValue(true)
    invoke.mockResolvedValue(0)
    await expect(subscribeSession(1, vi.fn(), vi.fn())).rejects.toThrow(
      'Invalid bridge port'
    )
    expect(TestEventSource.instances).toHaveLength(0)
  })
})

describe('session subscription', () => {
  it.each(['session', 'bridge-error'])(
    'closes on malformed %s JSON',
    async (event) => {
      const error = vi.fn()
      const receive = vi.fn()
      await subscribeSession(1, receive, error)
      current().dispatchEvent(new MessageEvent(event, { data: '{' }))
      current().dispatchEvent(new Event('error'))
      expect(current().close).toHaveBeenCalledTimes(1)
      expect(error).toHaveBeenCalledTimes(1)
      expect(receive).not.toHaveBeenCalled()
    }
  )

  it('delivers increasing snapshots and closes on the terminal snapshot', async () => {
    const receive = vi.fn()
    const error = vi.fn()
    const stop = await subscribeSession(1, receive, error)
    expect(current().url).toBe('http://localhost:1420/bridge/sessions/1/events')
    current().send('session', snapshot(0))
    current().send('session', snapshot(2))
    current().send('session', snapshot(1))
    current().send('session', snapshot(2))
    current().send('session', snapshot(3, 'Closed'))
    current().send('session', snapshot(4))
    current().dispatchEvent(new Event('error'))
    stop()
    expect(receive.mock.calls.map(([value]) => value.seq)).toEqual([0, 2, 3])
    expect(current().close).toHaveBeenCalledTimes(1)
    expect(error).not.toHaveBeenCalled()
  })

  it('releases the stream on abort without cancelling the operation', async () => {
    const controller = new AbortController()
    const receive = vi.fn()
    const error = vi.fn()
    const stop = await subscribeSession(1, receive, error, controller.signal)
    controller.abort()
    stop()
    current().send('session', snapshot())
    expect(current().close).toHaveBeenCalledTimes(1)
    expect(receive).not.toHaveBeenCalled()
    expect(error).not.toHaveBeenCalled()
    expect(invoke).not.toHaveBeenCalled()
  })

  it('does not open a connection after an abort during port lookup', async () => {
    isTauri.mockReturnValue(true)
    const controller = new AbortController()
    let resolvePort!: (port: number) => void
    invoke.mockReturnValue(
      new Promise<number>((resolve) => {
        resolvePort = resolve
      })
    )
    const subscription = subscribeSession(
      1,
      vi.fn(),
      vi.fn(),
      controller.signal
    )
    controller.abort()
    resolvePort(1421)
    await subscription
    expect(TestEventSource.instances).toHaveLength(0)
  })

  it('closes instead of automatically reconnecting on network failure', async () => {
    const error = vi.fn()
    await subscribeSession(1, vi.fn(), error)
    current().dispatchEvent(new Event('error'))
    current().dispatchEvent(new Event('error'))
    expect(current().close).toHaveBeenCalledTimes(1)
    expect(error).toHaveBeenCalledTimes(1)
    expect(error.mock.calls[0][0].message).toContain('disconnected')
  })

  it('reports a server stream error and releases the connection', async () => {
    const error = vi.fn()
    await subscribeSession(1, vi.fn(), error)
    current().send('bridge-error', {
      kind: 'internal_error',
      message: 'Unsafe snapshot'
    })
    expect(error.mock.calls[0][0].message).toBe('Unsafe snapshot')
    expect(current().close).toHaveBeenCalledTimes(1)
  })

  it.each([
    { session: 2, seq: 0, lifecycle: 'Open' },
    { session: 1, seq: Number.MAX_SAFE_INTEGER + 1, lifecycle: 'Open' },
    { session: 1, seq: 0, lifecycle: 'Unknown' },
    null
  ])('rejects an invalid snapshot %j', async (value) => {
    const error = vi.fn()
    const receive = vi.fn()
    await subscribeSession(1, receive, error)
    current().send('session', value)
    expect(receive).not.toHaveBeenCalled()
    expect(error).toHaveBeenCalledTimes(1)
    expect(current().close).toHaveBeenCalledTimes(1)
  })
})

describe('settings subscription', () => {
  const stored = (revision: number) =>
    ({ settings: {}, revision, problem: null }) as unknown as StoredSettings

  it('delivers increasing revisions without ending on its own', async () => {
    const receive = vi.fn()
    const error = vi.fn()
    const stop = await subscribeSettings(receive, error)
    expect(current().url).toBe('http://localhost:1420/bridge/settings/events')
    for (const revision of [0, 2, 1, 2, 3])
      current().send('settings', stored(revision))
    expect(receive.mock.calls.map(([value]) => value.revision)).toEqual([
      0, 2, 3
    ])
    expect(current().close).not.toHaveBeenCalled()
    stop()
    expect(current().close).toHaveBeenCalledTimes(1)
    expect(error).not.toHaveBeenCalled()
  })

  it.each([
    { settings: {}, revision: -1, problem: null },
    { settings: {}, revision: Number.MAX_SAFE_INTEGER + 1, problem: null },
    { settings: null, revision: 0, problem: null },
    null
  ])('rejects invalid settings %j', async (value) => {
    const error = vi.fn()
    const receive = vi.fn()
    await subscribeSettings(receive, error)
    current().send('settings', value)
    expect(receive).not.toHaveBeenCalled()
    expect(error).toHaveBeenCalledTimes(1)
    expect(current().close).toHaveBeenCalledTimes(1)
  })

  it('reports disconnects with the stream name', async () => {
    const error = vi.fn()
    await subscribeSettings(vi.fn(), error)
    current().dispatchEvent(new Event('error'))
    expect(error.mock.calls[0][0].message).toBe(
      'Settings subscription disconnected'
    )
  })
})
