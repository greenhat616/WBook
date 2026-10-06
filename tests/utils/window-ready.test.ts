// @vitest-environment jsdom

import { renderHook, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, test, vi } from 'vitest'
import { invoke as tauriInvoke, isTauri } from '@tauri-apps/api/core'
import { useWindowReady } from '../../src/hooks/use-window-ready'

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => false)
}))

afterEach(() => {
  vi.resetAllMocks()
  vi.restoreAllMocks()
})

describe('useWindowReady', () => {
  test('reports readiness once to the desktop host', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(tauriInvoke).mockResolvedValue(null)
    const { rerender } = renderHook(useWindowReady)
    rerender()
    await waitFor(() =>
      expect(tauriInvoke).toHaveBeenCalledWith('window_ready', {})
    )
    expect(tauriInvoke).toHaveBeenCalledTimes(1)
  })

  test('logs a failed report instead of throwing', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(tauriInvoke).mockRejectedValue(new Error('no window'))
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    renderHook(useWindowReady)
    await waitFor(() => expect(log).toHaveBeenCalled())
  })

  test('does nothing in the browser', () => {
    vi.mocked(isTauri).mockReturnValue(false)
    renderHook(useWindowReady)
    expect(tauriInvoke).not.toHaveBeenCalled()
  })
})
