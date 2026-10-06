import { invoke as tauriInvoke, isTauri } from '@tauri-apps/api/core'
import { DESKTOP_ONLY_COMMANDS, type CommandError } from './bindings'

function isCommandError(value: unknown): value is CommandError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'kind' in value &&
    typeof value.kind === 'string' &&
    'message' in value &&
    typeof value.message === 'string'
  )
}

function checkIntegers(value: unknown): void {
  if (typeof value === 'number' && !Number.isSafeInteger(value)) {
    throw new Error('Numeric values must be JavaScript safe integers')
  }
  if (value && typeof value === 'object') {
    for (const child of Object.values(value)) checkIntegers(child)
  }
}

export async function invoke<T>(
  method: string,
  params: Record<string, unknown> = {}
): Promise<T> {
  checkIntegers(params)
  if (isTauri()) {
    try {
      const value = await tauriInvoke<T>(method, params)
      checkIntegers(value)
      return value
    } catch (error) {
      if (error instanceof Error || isCommandError(error)) throw error
      throw new Error(String(error))
    }
  }

  if (DESKTOP_ONLY_COMMANDS.some((name) => name === method)) {
    throw {
      kind: 'platform_unsupported',
      message: 'This operation requires the desktop application'
    } satisfies CommandError
  }

  const response = await fetch(
    import.meta.env.VITE_WBOOK_RPC_URL || '/bridge/rpc',
    {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ method, params })
    }
  )
  let value: unknown
  try {
    value = await response.json()
  } catch {
    throw new Error(`RPC returned non-JSON data (HTTP ${response.status})`)
  }
  if (!response.ok) {
    if (response.status !== 403 && isCommandError(value)) throw value
    throw new Error(`RPC request failed (HTTP ${response.status})`)
  }
  checkIntegers(value)
  return value as T
}
