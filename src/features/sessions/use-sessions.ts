import { useCallback, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { mutations, queries } from '../../bindings'
import { errorMessage } from './api'
import { defaultParserConfig } from './parser-config'

export function useSessions() {
  const queryClient = useQueryClient()
  const list = useQuery(queries.listSessions())
  const creation = useMutation(mutations.createSession())
  const [createError, setCreateError] = useState<string | null>(null)
  const creating = useRef(false)

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
