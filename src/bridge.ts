import { isTauri } from '@tauri-apps/api/core'
import {
  commands,
  type PreviewInfo,
  type SessionSnapshot,
  type StoredSettings
} from './bindings'
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

type Stream<T> = {
  path: string
  event: string
  name: string
  // Throws on invalid data; returns the value's position in the stream.
  validate: (value: T) => number
  // Whether the value is the last one the stream will send.
  final?: (value: T) => boolean
}

async function subscribe<T>(
  stream: Stream<T>,
  onValue: (value: T) => void,
  onError: (error: Error) => void,
  signal?: AbortSignal
): Promise<() => void> {
  if (signal?.aborted) return () => {}
  const url = await bridgeUrl(stream.path)
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
  source.addEventListener(stream.event, (event) => {
    if (stopped) return
    let value: T
    let position: number
    try {
      value = JSON.parse((event as MessageEvent<string>).data)
      checkIntegers(value)
      position = stream.validate(value)
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)))
      return
    }
    if (position <= sequence) return
    sequence = position
    if (stream.final?.(value)) stop()
    onValue(value)
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
            : `${stream.name} subscription failed`
        )
      )
    } catch {
      fail(new Error(`${stream.name} subscription returned invalid data`))
    }
  })
  source.addEventListener('error', () => {
    fail(new Error(`${stream.name} subscription disconnected`))
  })
  signal?.addEventListener('abort', stop, { once: true })
  return stop
}

export async function subscribeSession(
  sessionId: number,
  onSnapshot: (snapshot: SessionSnapshot) => void,
  onError: (error: Error) => void,
  signal?: AbortSignal
): Promise<() => void> {
  checkSessionId(sessionId)
  return subscribe<SessionSnapshot>(
    {
      path: `/bridge/sessions/${sessionId}/events`,
      event: 'session',
      name: 'Session',
      validate: (snapshot) => {
        if (
          !snapshot ||
          snapshot.session !== sessionId ||
          !Number.isSafeInteger(snapshot.seq) ||
          snapshot.seq < 0 ||
          !['Open', 'Closing', 'Closed'].includes(snapshot.lifecycle)
        ) {
          throw new Error('Invalid session snapshot')
        }
        return snapshot.seq
      },
      final: (snapshot) => snapshot.lifecycle === 'Closed'
    },
    onSnapshot,
    onError,
    signal
  )
}

/** Follows the global settings; the first value is the current one. */
export function subscribeSettings(
  onSettings: (stored: StoredSettings) => void,
  onError: (error: Error) => void,
  signal?: AbortSignal
): Promise<() => void> {
  return subscribe<StoredSettings>(
    {
      path: '/bridge/settings/events',
      event: 'settings',
      name: 'Settings',
      validate: (stored) => {
        if (
          !stored ||
          typeof stored.settings !== 'object' ||
          stored.settings === null ||
          !Number.isSafeInteger(stored.revision) ||
          stored.revision < 0
        ) {
          throw new Error('Invalid settings')
        }
        return stored.revision
      }
    },
    onSettings,
    onError,
    signal
  )
}
