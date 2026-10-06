import { useCallback, useEffect, useRef, useState } from 'react'
import { commands, type SessionSnapshot } from '../../bindings'
import { errorMessage, unwrap } from './api'

export function useSessions() {
  const [sessions, setSessions] = useState<SessionSnapshot[]>([])
  const [loading, setLoading] = useState(true)
  const [pending, setPending] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const context = useRef({ alive: false, request: 0, creating: false })

  const refresh = useCallback(async () => {
    const current = context.current
    const request = ++current.request
    setLoading(true)
    setError(null)
    try {
      const sessions = unwrap(await commands.listSessions())
      if (current.alive && current.request === request) setSessions(sessions)
    } catch (error) {
      if (current.alive && current.request === request)
        setError(errorMessage(error))
    } finally {
      if (current.alive && current.request === request) setLoading(false)
    }
  }, [])

  useEffect(() => {
    const current = { alive: true, request: 0, creating: false }
    context.current = current
    void refresh()
    return () => {
      current.alive = false
    }
  }, [refresh])

  const create = useCallback(async (source: string, parts: number) => {
    const current = context.current
    if (!current.alive || current.creating) return null
    current.creating = true
    setPending(true)
    setError(null)
    try {
      if (!source.trim()) throw new Error('请输入本机文本文件路径')
      if (!Number.isSafeInteger(parts) || parts < 1) {
        throw new Error('分段数必须是正整数')
      }
      const session = unwrap(
        await commands.createSession(source.trim(), {
          filters: [],
          toc: { SplitEvenly: { parts } }
        })
      )
      if (!current.alive) return null
      // Invalidate a list request started before creation so it cannot hide the new session.
      current.request++
      setLoading(false)
      setSessions((sessions) => [
        ...sessions.filter((item) => item.session !== session.session),
        session
      ])
      return session
    } catch (error) {
      if (current.alive) setError(errorMessage(error))
      return null
    } finally {
      current.creating = false
      if (current.alive) setPending(false)
    }
  }, [])

  return { sessions, loading, pending, error, refresh, create }
}
