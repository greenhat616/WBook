import { useCallback, useEffect, useRef, useState } from 'react'
import {
  commands,
  type CleanupWarning,
  type ClosedSession,
  type OperationResponse,
  type Outcome,
  type PreviewInfo,
  type SessionSnapshot,
  type WorkspaceResults
} from '../../bindings'
import { subscribeSession } from '../../bridge'
import { errorMessage, exportOptions, unwrap } from './api'

type State = {
  snapshot: SessionSnapshot | null
  results: WorkspaceResults | null
  preview: PreviewInfo | null
  error: string | null
  warnings: CleanupWarning[]
  loading: boolean
  pending: boolean
  connection: 'connecting' | 'live' | 'disconnected' | 'closed'
  notice: string | null
  exportPath: string | null
}

type Context = {
  id: number
  alive: boolean
  busy: boolean
  closing: boolean
  closed: boolean
  snapshot: SessionSnapshot | null
  resultsRevision: number | null
  controller: AbortController
}

const initialState = (): State => ({
  snapshot: null,
  results: null,
  preview: null,
  error: null,
  warnings: [],
  loading: true,
  pending: false,
  connection: 'connecting',
  notice: null,
  exportPath: null
})

export function useSession(sessionId: number) {
  const [state, setState] = useState(initialState)
  const context = useRef<Context | null>(null)

  const patch = useCallback((current: Context, update: Partial<State>) => {
    if (current.alive) setState((state) => ({ ...state, ...update }))
  }, [])

  const warnings = useCallback(
    (current: Context, warnings: CleanupWarning[]) => {
      if (current.alive && warnings.length) {
        setState((state) => ({
          ...state,
          warnings: [...state.warnings, ...warnings]
        }))
      }
    },
    []
  )

  const applySnapshot = useCallback(
    (current: Context, snapshot: SessionSnapshot) => {
      if (
        !current.alive ||
        snapshot.session !== current.id ||
        (current.snapshot && snapshot.seq <= current.snapshot.seq)
      )
        return
      current.snapshot = snapshot
      const available =
        snapshot.lifecycle === 'Open'
          ? snapshot.workspace_status.Available
          : undefined
      if (snapshot.lifecycle === 'Closed') current.closed = true
      setState((state) => ({
        ...state,
        snapshot,
        results:
          available && available.revision === current.resultsRevision
            ? state.results
            : null,
        preview:
          available &&
          state.preview &&
          available.revision === state.preview.revision &&
          available.preview_id === state.preview.id
            ? state.preview
            : null,
        connection:
          snapshot.lifecycle === 'Closed' ? 'closed' : state.connection
      }))
    },
    []
  )

  const read = useCallback(
    async (current: Context) => {
      const snapshot = unwrap(await commands.getSession(current.id))
      if (!current.alive || current.closing || current.closed) return
      applySnapshot(current, snapshot)
      const latest = current.snapshot!
      const available = latest.workspace_status.Available
      if (
        latest.lifecycle !== 'Open' ||
        latest.activity !== 'Idle' ||
        !available ||
        available.document === 'Absent'
      )
        return
      const response = unwrap(await commands.readResults(current.id))
      if (!current.alive || current.closing || current.closed) return
      warnings(current, response.warnings)
      const results = unwrap(response.outcome, '操作')
      const after = unwrap(await commands.getSession(current.id))
      if (!current.alive || current.closing || current.closed) return
      applySnapshot(current, after)
      if (
        current.snapshot?.lifecycle === 'Open' &&
        current.snapshot?.workspace_status.Available?.revision ===
          response.revision
      ) {
        current.resultsRevision = response.revision
        patch(current, { results })
      }
    },
    [applySnapshot, patch, warnings]
  )

  const refresh = useCallback(async () => {
    const current = context.current
    if (!current?.alive || current.busy || current.closing || current.closed)
      return
    current.busy = true
    patch(current, { loading: true, error: null })
    try {
      await read(current)
    } catch (error) {
      if (!current.closing && !current.closed)
        patch(current, { error: errorMessage(error) })
    } finally {
      current.busy = false
      patch(current, { loading: false })
    }
  }, [patch, read])

  const connect = useCallback(
    (current: Context) => {
      current.controller.abort()
      const controller = new AbortController()
      current.controller = controller
      patch(current, { connection: 'connecting', error: null })
      void subscribeSession(
        current.id,
        (snapshot) => {
          if (controller.signal.aborted || !current.alive) return
          patch(current, {
            connection: snapshot.lifecycle === 'Closed' ? 'closed' : 'live'
          })
          applySnapshot(current, snapshot)
        },
        (error) => {
          if (!controller.signal.aborted && !current.closed) {
            patch(current, {
              connection: 'disconnected',
              error: errorMessage(error)
            })
          }
        },
        controller.signal
      )
        .then((stop) => {
          if (controller.signal.aborted) stop()
        })
        .catch((error: unknown) => {
          if (!controller.signal.aborted && !current.closed) {
            patch(current, {
              connection: 'disconnected',
              error: errorMessage(error)
            })
          }
        })
    },
    [applySnapshot, patch]
  )

  useEffect(() => {
    const current: Context = {
      id: sessionId,
      alive: true,
      busy: false,
      closing: false,
      closed: false,
      snapshot: null,
      resultsRevision: null,
      controller: new AbortController()
    }
    context.current = current
    setState(initialState())
    connect(current)
    void refresh()
    return () => {
      current.alive = false
      current.controller.abort()
    }
  }, [connect, refresh, sessionId])

  const operate = useCallback(
    async <T>(
      command: (current: Context) => Promise<Outcome<OperationResponse<T>>>,
      completed: (current: Context, data: T) => void
    ) => {
      const current = context.current
      if (!current?.alive || current.busy || current.closing || current.closed)
        return
      if (
        current.snapshot?.activity !== 'Idle' ||
        current.snapshot.lifecycle !== 'Open' ||
        !current.snapshot.workspace_status.Available
      ) {
        patch(current, { error: '会话正在处理其他操作，请稍后重试' })
        return
      }
      current.busy = true
      patch(current, { pending: true, error: null, notice: null })
      try {
        const response = unwrap(await command(current))
        if (!current.alive || current.closing || current.closed) return
        warnings(current, response.warnings)
        const data = unwrap(response.outcome, '操作')
        try {
          await read(current)
        } catch (error) {
          if (!current.closing && !current.closed)
            patch(current, { error: errorMessage(error) })
        }
        if (current.alive && !current.closing && !current.closed)
          completed(current, data)
      } catch (error) {
        if (!current.closing && !current.closed)
          patch(current, { error: errorMessage(error) })
      } finally {
        current.busy = false
        if (!current.closing) patch(current, { pending: false })
      }
    },
    [patch, read, warnings]
  )

  const initialize = useCallback(
    () =>
      operate(
        (current) => commands.initializeSession(current.id),
        (current) => patch(current, { notice: '初始化完成' })
      ),
    [operate, patch]
  )

  const renderPreview = useCallback(
    () =>
      operate(
        (current) =>
          commands.renderPreview(
            current.id,
            current.snapshot!.workspace_status.Available!.revision,
            exportOptions
          ),
        (current, preview) => {
          const available = current.snapshot?.workspace_status.Available
          if (
            available?.revision === preview.revision &&
            available.preview_id === preview.id
          ) {
            patch(current, { preview, notice: '预览已生成' })
          }
        }
      ),
    [operate, patch]
  )

  const exportBook = useCallback(
    (destination: string) =>
      operate(
        (current) => {
          if (!destination.trim()) throw new Error('请输入 EPUB 导出路径')
          return commands.exportEpub(
            current.id,
            current.snapshot!.workspace_status.Available!.revision,
            exportOptions,
            destination.trim()
          )
        },
        (current, exported) => {
          warnings(current, exported.cleanup_failures)
          patch(current, { exportPath: exported.path, notice: 'EPUB 已导出' })
        }
      ),
    [operate, patch, warnings]
  )

  const cancel = useCallback(async () => {
    const current = context.current
    if (!current?.alive || current.closing || current.closed) return
    try {
      const snapshot = unwrap(await commands.getSession(current.id))
      if (!current.alive || current.closing || current.closed) return
      applySnapshot(current, snapshot)
      if (snapshot.activity === 'Idle') {
        patch(current, { notice: '当前没有可取消的操作' })
        return
      }
      const reply = unwrap(
        await commands.cancelOperation(current.id, snapshot.activity.Running.op)
      )
      if (!current.closing && !current.closed) {
        patch(current, {
          notice:
            reply === 'Requested'
              ? '已请求取消，正在等待操作结束'
              : '操作已结束，无需取消'
        })
      }
    } catch (error) {
      if (!current.closing && !current.closed)
        patch(current, { error: errorMessage(error) })
    }
  }, [applySnapshot, patch])

  const close = useCallback(async (): Promise<ClosedSession | null> => {
    const current = context.current
    if (!current?.alive || current.closing || current.closed) return null
    current.closing = true
    patch(current, { pending: true, error: null })
    try {
      const report = unwrap(await commands.closeSession(current.id))
      if (!current.alive) return null
      warnings(current, report.cleanup_failures)
      current.closed = true
      current.controller.abort()
      patch(current, {
        connection: 'closed',
        pending: false,
        results: null,
        preview: null,
        notice: '会话已关闭'
      })
      return report
    } catch (error) {
      patch(current, { error: errorMessage(error), pending: current.busy })
      return null
    } finally {
      current.closing = false
    }
  }, [patch, warnings])

  const reconnect = useCallback(() => {
    const current = context.current
    if (current?.alive && !current.closed && !current.closing) connect(current)
  }, [connect])

  return {
    ...state,
    refresh,
    reconnect,
    initialize,
    renderPreview,
    exportBook,
    cancel,
    close
  }
}
