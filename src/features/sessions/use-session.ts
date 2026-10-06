import { useCallback, useEffect, useRef, useState } from 'react'
import {
  commands,
  type CleanupWarning,
  type ClosedSession,
  type DocumentVersion,
  type Metadata,
  type OperationResponse,
  type ParsedResults_Serialize,
  type PreviewInfo,
  type SessionSnapshot,
  type Settings,
  type TextRange,
  type TocSettings,
  type WorkspaceResults
} from '../../bindings'
import { subscribeSession } from '../../bridge'
import { errorMessage, unwrap } from './api'
import { sameToc } from './parser-config'

type State = {
  snapshot: SessionSnapshot | null
  results: WorkspaceResults | null
  settings: Settings | null
  preview: PreviewInfo | null
  // A parse result awaiting review; installing it replaces the current TOC.
  draft: ParsedResults_Serialize | null
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
  settings: Settings | null
  // The revision whose settings were last requested.
  settingsRevision: number | null
  controller: AbortController
}

const sameVersion = (
  a: DocumentVersion | null | undefined,
  b: DocumentVersion | null | undefined
) =>
  !!a &&
  !!b &&
  a.revision === b.revision &&
  a.document_id.every((byte, index) => byte === b.document_id[index])

const initialState = (): State => ({
  snapshot: null,
  results: null,
  settings: null,
  preview: null,
  draft: null,
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

  // Settings stay readable while an operation runs, so they are fetched
  // whenever the revision moves instead of waiting for the session to idle.
  const loadSettings = useCallback(
    async (current: Context, revision: number) => {
      current.settingsRevision = revision
      try {
        const settings = await commands.getSessionSettings(current.id)
        if (current.alive && current.settingsRevision === revision) {
          current.settings = settings
          patch(current, { settings })
        }
      } catch {
        // A closing or lost session rejects the read; its snapshot says so.
      }
    },
    [patch]
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
      if (available && available.revision !== current.settingsRevision)
        void loadSettings(current, available.revision)
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
        // Parsing reads one document version; any edit makes the draft unusable.
        draft:
          available &&
          state.draft &&
          sameVersion(available.document_version, state.draft.version)
            ? state.draft
            : null,
        connection:
          snapshot.lifecycle === 'Closed' ? 'closed' : state.connection
      }))
    },
    [loadSettings]
  )

  const read = useCallback(
    async (current: Context) => {
      const snapshot = await commands.getSession(current.id)
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
      const response = await commands.readResults(current.id)
      if (!current.alive || current.closing || current.closed) return
      warnings(current, response.warnings)
      const results = unwrap(response.outcome, '操作')
      const after = await commands.getSession(current.id)
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
      settings: null,
      settingsRevision: null,
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
      command: (current: Context) => Promise<OperationResponse<T>>,
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
        const response = await command(current)
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
            current.snapshot!.workspace_status.Available!.revision
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

  // Parsing always uses the session's saved rules, so changed rules are
  // saved first.
  const parse = useCallback(
    (toc: TocSettings) =>
      operate(
        async (current) => {
          const settings = current.settings
          if (!settings) throw new Error('本书设置尚未加载，请稍后重试')
          if (!sameToc(settings.toc, toc)) {
            const saved = await commands.setSessionSettings(
              current.id,
              current.snapshot!.workspace_status.Available!.revision,
              { ...settings, toc }
            )
            warnings(current, saved.warnings)
            unwrap(saved.outcome, '保存解析规则')
          }
          return commands.parseSession(current.id)
        },
        (current, draft) => {
          const available = current.snapshot?.workspace_status.Available
          if (sameVersion(available?.document_version, draft.version)) {
            patch(current, { draft, notice: '试解析完成，确认后应用' })
          }
        }
      ),
    [operate, patch, warnings]
  )

  const install = useCallback(
    (draft: ParsedResults_Serialize) =>
      operate(
        (current) =>
          commands.installResults(
            current.id,
            current.snapshot!.workspace_status.Available!.revision,
            draft
          ),
        (current) => patch(current, { draft: null, notice: '已应用新的目录' })
      ),
    [operate, patch]
  )

  const discardDraft = useCallback(() => {
    const current = context.current
    if (current) patch(current, { draft: null, notice: null })
  }, [patch])

  const setOverrides = useCallback(
    (overrides: Metadata) =>
      operate(
        (current) =>
          commands.setMetadataOverrides(
            current.id,
            current.snapshot!.workspace_status.Available!.revision,
            overrides
          ),
        (current) => patch(current, { notice: '书籍信息已更新' })
      ),
    [operate, patch]
  )

  // Reads bypass the pending flag: they never change the session, and the
  // backend reports `busy` itself if an operation is still running.
  const readText = useCallback(
    async (version: DocumentVersion, range: TextRange) => {
      const current = context.current
      if (!current?.alive) throw new Error('会话已失效')
      const response = await commands.readText(current.id, version, range)
      warnings(current, response.warnings)
      return unwrap(response.outcome, '读取正文')
    },
    [warnings]
  )

  const cancel = useCallback(async () => {
    const current = context.current
    if (!current?.alive || current.closing || current.closed) return
    try {
      const snapshot = await commands.getSession(current.id)
      if (!current.alive || current.closing || current.closed) return
      applySnapshot(current, snapshot)
      if (snapshot.activity === 'Idle') {
        patch(current, { notice: '当前没有可取消的操作' })
        return
      }
      const reply = await commands.cancelOperation(
        current.id,
        snapshot.activity.Running.op
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
      const report = await commands.closeSession(current.id)
      if (!current.alive) return null
      warnings(current, report.cleanup_failures)
      current.closed = true
      current.controller.abort()
      patch(current, {
        connection: 'closed',
        pending: false,
        results: null,
        preview: null,
        draft: null,
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
    parse,
    install,
    discardDraft,
    setOverrides,
    readText,
    cancel,
    close
  }
}
