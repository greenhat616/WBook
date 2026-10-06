import { useCallback, useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { mutations, queries } from '../../bindings'
import { errorMessage } from './api'
import { defaultParserConfig } from './parser-config'

export function useSessions() {
  const queryClient = useQueryClient()
  const list = useQuery(queries.listSessions())
  const creation = useMutation(mutations.createSession())
  const [createError, setCreateError] = useState<string | null>(null)
  const creating = useRef(false)

  // Session windows close their sessions on the host, which announces it here
  // (SESSION_CLOSED_EVENT in backend/tauri/src/windows.rs). The session is
  // dropped directly: the close is published before deregistration, so an
  // immediate refetch could still list it.
  useEffect(() => {
    if (!isTauri()) return
    let active = true
    let unlisten: (() => void) | undefined
    void getCurrentWebview()
      .listen<number>('session-closed', ({ payload }) => {
        const { queryKey } = queries.listSessions()
        void queryClient
          .cancelQueries({ queryKey })
          .then(() =>
            queryClient.setQueryData(queryKey, (sessions) =>
              sessions?.filter((item) => item.session !== payload)
            )
          )
      })
      .then((stop) => {
        if (active) unlisten = stop
        else stop()
      })
    return () => {
      active = false
      unlisten?.()
    }
  }, [queryClient])

  const refresh = useCallback(async () => {
    setCreateError(null)
    await list.refetch()
  }, [list])

  const create = useCallback(
    async (source: string) => {
      if (creating.current) return null
      creating.current = true
      setCreateError(null)
      try {
        if (!source.trim()) throw new Error('请输入本机文本文件路径')
        const session = await creation.mutateAsync({
          source: source.trim(),
          // Chapters can be re-parsed with other rules from the session page.
          options: { filters: [], toc: defaultParserConfig }
        })
        const { queryKey } = queries.listSessions()
        // Drop a list request started before creation so it cannot hide the new session.
        await queryClient.cancelQueries({ queryKey })
        queryClient.setQueryData(queryKey, (sessions = []) => [
          ...sessions.filter((item) => item.session !== session.session),
          session
        ])
        return session
      } catch (error) {
        setCreateError(errorMessage(error))
        return null
      } finally {
        creating.current = false
      }
    },
    [creation, queryClient]
  )

  return {
    sessions: list.data ?? [],
    loading: list.isFetching,
    pending: creation.isPending,
    error: createError ?? (list.error ? errorMessage(list.error) : null),
    refresh,
    create
  }
}
