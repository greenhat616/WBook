import { useEffect } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { commands } from '@/bindings'

// Desktop windows start hidden so they never flash blank; the host shows the
// window once its first screen has rendered.
export function useWindowReady(): void {
  useEffect(() => {
    if (!isTauri()) return
    commands.windowReady().catch((error: unknown) => {
      // The host still shows the window after a timeout.
      console.error('Could not report window readiness', error)
    })
  }, [])
}
