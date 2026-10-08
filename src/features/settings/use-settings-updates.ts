import { useEffect, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { queries, type StoredSettings } from '@/bindings'
import { subscribeSettings } from '@/bridge'

/**
 * Keeps the cached global settings current while other windows save them.
 * Returns why updates stopped arriving, and a way to reconnect.
 */
export function useSettingsUpdates() {
  const queryClient = useQueryClient()
  const [error, setError] = useState<string | null>(null)
  const [attempt, setAttempt] = useState(0)

  useEffect(() => {
    const controller = new AbortController()
    const { queryKey } = queries.getSettings()
    const apply = (stored: StoredSettings) =>
      queryClient.setQueryData(queryKey, (cached) =>
        // A save's own response may land after the stream announced it.
        cached && cached.revision > stored.revision ? cached : stored
      )
    subscribeSettings(
      apply,
      (cause) => setError(cause.message),
      controller.signal
    ).catch((cause: unknown) =>
      setError(cause instanceof Error ? cause.message : String(cause))
    )
    return () => controller.abort()
  }, [queryClient, attempt])

  return {
    error,
    reconnect() {
      setError(null)
      setAttempt((value) => value + 1)
    }
  }
}
