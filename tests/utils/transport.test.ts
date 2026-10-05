import { afterEach, describe, expect, test, vi } from 'vitest'
import { invoke as tauriInvoke, isTauri } from '@tauri-apps/api/core'
import { commands } from '../../src/bindings'
import { invoke } from '../../src/transport'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => false)
}))

afterEach(() => {
  vi.resetAllMocks()
  vi.unstubAllGlobals()
  vi.unstubAllEnvs()
})

describe('generated commands share the transport contract', () => {
  test('desktop invokes the original command and camelCase arguments', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(tauriInvoke).mockResolvedValue('NotActive')
    expect(await commands.cancelOperation(1, 2)).toBe('NotActive')
    expect(tauriInvoke).toHaveBeenCalledWith('cancel_operation', {
      sessionId: 1,
      operationId: 2
    })
  })

  test('browser posts to the configured endpoint and returns the bare value', async () => {
    vi.stubEnv('VITE_WBOOK_RPC_URL', 'http://127.0.0.1:1421/bridge/rpc')
    const fetch = vi.fn().mockResolvedValue(Response.json('NotActive'))
    vi.stubGlobal('fetch', fetch)
    expect(await commands.cancelOperation(1, 2)).toBe('NotActive')
    expect(fetch).toHaveBeenCalledWith('http://127.0.0.1:1421/bridge/rpc', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        method: 'cancel_operation',
        params: { sessionId: 1, operationId: 2 }
      })
    })
  })

  test('browser no-argument calls use an empty object and same-origin fallback', async () => {
    vi.stubEnv('VITE_WBOOK_RPC_URL', '')
    const fetch = vi.fn().mockResolvedValue(Response.json([]))
    vi.stubGlobal('fetch', fetch)
    expect(await commands.listSessions()).toEqual([])
    expect(fetch.mock.calls[0][0]).toBe('/bridge/rpc')
    expect(JSON.parse(fetch.mock.calls[0][1].body)).toEqual({
      method: 'list_sessions',
      params: {}
    })
  })

  test.each([true, false])(
    'domain errors keep their shape with desktop=%s',
    async (desktop) => {
      const error = { kind: 'not_found', message: 'session not found' }
      vi.mocked(isTauri).mockReturnValue(desktop)
      vi.mocked(tauriInvoke).mockRejectedValue(error)
      vi.stubGlobal(
        'fetch',
        vi.fn().mockResolvedValue(Response.json(error, { status: 404 }))
      )
      await expect(commands.getSession(1)).rejects.toEqual(error)
    }
  )

  test('desktop-only commands never send HTTP requests', async () => {
    const fetch = vi.fn()
    vi.stubGlobal('fetch', fetch)
    await expect(commands.getPort()).rejects.toMatchObject({
      kind: 'platform_unsupported'
    })
    expect(fetch).not.toHaveBeenCalled()
  })

  test('desktop get_port retains its bare number return', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(tauriInvoke).mockResolvedValue(1421)
    expect(await commands.getPort()).toBe(1421)
  })

  test.each([200, 502])(
    'non-JSON HTTP %s stays a transport rejection',
    async (status) => {
      vi.stubGlobal(
        'fetch',
        vi
          .fn()
          .mockResolvedValue(new Response('<html>proxy</html>', { status }))
      )
      await expect(commands.listSessions()).rejects.toThrow(`HTTP ${status}`)
    }
  )

  test('HTTP denial, malformed errors, network errors and IPC strings are transport rejections', async () => {
    const fetch = vi
      .fn()
      .mockResolvedValueOnce(
        Response.json({ kind: 'forbidden', message: 'Denied' }, { status: 403 })
      )
      .mockResolvedValueOnce(
        Response.json({ detail: 'Bad gateway' }, { status: 502 })
      )
      .mockRejectedValueOnce(new TypeError('Network unavailable'))
    vi.stubGlobal('fetch', fetch)
    await expect(commands.listSessions()).rejects.toThrow('HTTP 403')
    await expect(commands.listSessions()).rejects.toThrow('HTTP 502')
    await expect(commands.listSessions()).rejects.toThrow('Network unavailable')
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(tauriInvoke).mockRejectedValue('invalid args')
    await expect(commands.listSessions()).rejects.toThrow('invalid args')
  })

  test('unsafe inputs are rejected before transport and unsafe outputs reject', async () => {
    const fetch = vi
      .fn()
      .mockResolvedValue(Response.json({ revision: 2 ** 53 }))
    vi.stubGlobal('fetch', fetch)
    await expect(commands.getSession(2 ** 53)).rejects.toThrow('safe integers')
    expect(fetch).not.toHaveBeenCalled()
    await expect(invoke('get_session', { sessionId: 1 })).rejects.toThrow(
      'safe integers'
    )
  })
})
