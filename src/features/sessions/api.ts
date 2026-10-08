import type { CommandError, Outcome } from '../../bindings'

export function unwrap<T>(result: Outcome<T>, stage = '命令'): T {
  if (result.status === 'error') {
    throw new Error(
      `${stage}失败 (${result.error.kind})：${result.error.message}`
    )
  }
  return result.data
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const { kind, message } = error as CommandError
    return `命令失败 (${kind})：${message}`
  }
  return String(error)
}

/** The session was running another operation and rejected this one. */
export function isBusy(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    (error as Partial<CommandError>).kind === 'busy'
  )
}
