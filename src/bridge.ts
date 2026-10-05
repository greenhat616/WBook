import { isTauri } from '@tauri-apps/api/core'
import { commands, type PreviewInfo, type SessionSnapshot } from './bindings'
import { checkIntegers } from './transport'

function checkSessionId(sessionId: number): void {
  if (!Number.isSafeInteger(sessionId) || sessionId < 1) {
    throw new Error('Invalid session ID')
  }
}

async function bridgeUrl(path: string): Promise<string> {
  if (isTauri()) {
    const port = await commands.getPort()
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      throw new Error('Invalid bridge port')
    }
    return new URL(path, `http://127.0.0.1:${port}`).href
  }
  const rpc = new URL(
    import.meta.env.VITE_WBOOK_RPC_URL || '/bridge/rpc',
    window.location.href
  )
  return new URL(path, rpc).href
}

export async function previewUrl(
  sessionId: number,
  preview: PreviewInfo,
  resource: string
): Promise<string> {
  checkSessionId(sessionId)
  const path = resource.split('/').map(encodeURIComponent).join('/')
  return bridgeUrl(
    `/bridge/preview/${sessionId}/${encodeURIComponent(preview.id)}/${path}`
  )
}

export async function subscribeSession(
  sessionId: number,
  onSnapshot: (snapshot: SessionSnapshot) => void,
  onError: (error: Error) => void,
  signal?: AbortSignal
): Promise<() => void> {
  checkSessionId(sessionId)
  if (signal?.aborted) return () => {}
  const url = await bridgeUrl(`/bridge/sessions/${sessionId}/events`)
  if (signal?.aborted) return () => {}
  const source = new EventSource(url)
  let stopped = false
  let sequence = -1
  const stop = () => {
    if (stopped) return
    stopped = true
    source.close()
    signal?.removeEventListener('abort', stop)
  }
  const fail = (error: Error) => {
    if (stopped) return
    stop()
    onError(error)
  }
  source.addEventListener('session', (event) => {
    if (stopped) return
    let snapshot: SessionSnapshot
    try {
      snapshot = JSON.parse((event as MessageEvent<string>).data)
      checkIntegers(snapshot)
      if (
        !snapshot ||
        snapshot.session !== sessionId ||
        !Number.isSafeInteger(snapshot.seq) ||
        snapshot.seq < 0 ||
        !['Open', 'Closing', 'Closed'].includes(snapshot.lifecycle)
      ) {
        throw new Error('Invalid session snapshot')
      }
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)))
      return
    }
    if (snapshot.seq <= sequence) return
    sequence = snapshot.seq
    if (snapshot.lifecycle === 'Closed') stop()
    onSnapshot(snapshot)
  })
  source.addEventListener('bridge-error', (event) => {
    try {
      const error: unknown = JSON.parse((event as MessageEvent<string>).data)
      fail(
        new Error(
          typeof error === 'object' &&
            error !== null &&
            'message' in error &&
            typeof error.message === 'string'
            ? error.message
            : 'Session subscription failed'
        )
      )
    } catch {
      fail(new Error('Session subscription returned invalid data'))
    }
  })
  source.addEventListener('error', () => {
    fail(new Error('Session subscription disconnected'))
  })
  signal?.addEventListener('abort', stop, { once: true })
  return stop
}
