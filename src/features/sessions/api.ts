import type { CommandError, ExportOptions, Outcome } from '../../bindings'

export const exportOptions: ExportOptions = {
  render: { layout: 'SingleHtml' },
  format: 'Epub',
  language: 'zh-CN',
  identifier: null
}

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
    return String((error as CommandError).message)
  }
  return String(error)
}
