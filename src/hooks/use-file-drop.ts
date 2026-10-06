import { useEffect, useRef, useState } from 'react'
import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'

type Handlers = {
  onPaths: (paths: string[]) => void
  // Browsers never expose a dropped file's path, which sessions need.
  onUnsupported: () => void
}

/** Tracks files dragged over the window; returns whether a drag is active. */
export function useFileDrop(handlers: Handlers): boolean {
  const [dragging, setDragging] = useState(false)
  const latest = useRef(handlers)
  latest.current = handlers

  useEffect(() => {
    if (isTauri()) {
      let active = true
      let unlisten: (() => void) | undefined
      // The desktop webview intercepts file drops natively, so DOM drag events never fire.
      void getCurrentWebview()
        .onDragDropEvent(({ payload }) => {
          if (payload.type === 'enter') setDragging(payload.paths.length > 0)
          else if (payload.type === 'leave') setDragging(false)
          else if (payload.type === 'drop') {
            setDragging(false)
            if (payload.paths.length) latest.current.onPaths(payload.paths)
          }
        })
        .then((stop) => {
          if (active) unlisten = stop
          else stop()
        })
      return () => {
        active = false
        unlisten?.()
      }
    }

    // dragenter/dragleave fire for every child element crossed, so count them.
    let depth = 0
    const hasFiles = (event: DragEvent) =>
      event.dataTransfer?.types.includes('Files') ?? false
    const enter = (event: DragEvent) => {
      if (!hasFiles(event)) return
      depth += 1
      setDragging(true)
    }
    const over = (event: DragEvent) => {
      // Without this the browser navigates away to the dropped file.
      if (hasFiles(event)) event.preventDefault()
    }
    const leave = (event: DragEvent) => {
      if (!hasFiles(event)) return
      depth = Math.max(0, depth - 1)
      if (depth === 0) setDragging(false)
    }
    const drop = (event: DragEvent) => {
      if (!hasFiles(event)) return
      event.preventDefault()
      depth = 0
      setDragging(false)
      latest.current.onUnsupported()
    }
    window.addEventListener('dragenter', enter)
    window.addEventListener('dragover', over)
    window.addEventListener('dragleave', leave)
    window.addEventListener('drop', drop)
    return () => {
      window.removeEventListener('dragenter', enter)
      window.removeEventListener('dragover', over)
      window.removeEventListener('dragleave', leave)
      window.removeEventListener('drop', drop)
    }
  }, [])

  return dragging
}
