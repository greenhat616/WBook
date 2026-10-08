// @vitest-environment jsdom

import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider
} from '@tanstack/react-router'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const { commands, isTauri } = vi.hoisted(() => {
  // jsdom lacks custom state sets, which the M3E elements toggle on connect.
  if (!('states' in ElementInternals.prototype)) {
    const states = new WeakMap<ElementInternals, Set<string>>()
    Object.defineProperty(ElementInternals.prototype, 'states', {
      get(this: ElementInternals) {
        if (!states.has(this)) states.set(this, new Set())
        return states.get(this)
      }
    })
  }
  return {
    commands: { openSettingsWindow: vi.fn() },
    isTauri: vi.fn()
  }
})

vi.mock('../../src/transport', () => ({
  invoke: (method: string) =>
    commands[
      method.replace(/_(\w)/g, (_, c: string) =>
        c.toUpperCase()
      ) as keyof typeof commands
    ]()
}))
vi.mock('@tauri-apps/api/core', () => ({ isTauri }))

import { AppShell } from '../../src/components/app-shell'

function renderShell(path: string) {
  const root = createRootRoute({
    component: () => (
      <AppShell>
        <Outlet />
      </AppShell>
    )
  })
  const router = createRouter({
    routeTree: root.addChildren(
      [
        '/',
        '/settings',
        '/sessions/$sessionId',
        '/sessions/$sessionId/settings'
      ].map((route) =>
        createRoute({
          getParentRoute: () => root,
          path: route,
          component: () => <h1>{route}</h1>
        })
      )
    ),
    history: createMemoryHistory({ initialEntries: [path] })
  })
  render(<RouterProvider router={router} />)
  return router
}

beforeEach(() => {
  vi.resetAllMocks()
  vi.spyOn(window, 'scrollTo').mockImplementation(() => {})
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('app shell', () => {
  it('opens the global settings window on the desktop', async () => {
    isTauri.mockReturnValue(true)
    commands.openSettingsWindow.mockResolvedValue(null)
    const router = renderShell('/sessions/1')
    await screen.findByRole('heading', { name: '/sessions/$sessionId' })
    fireEvent.click(screen.getByRole('button', { name: '设置' }))
    await waitFor(() =>
      expect(commands.openSettingsWindow).toHaveBeenCalledTimes(1)
    )
    expect(router.state.location.pathname).toBe('/sessions/1')
  })

  it('reports a settings window that cannot be opened', async () => {
    isTauri.mockReturnValue(true)
    commands.openSettingsWindow.mockRejectedValue({
      kind: 'internal_error',
      message: 'window failed'
    })
    renderShell('/')
    fireEvent.click(await screen.findByRole('button', { name: '设置' }))
    expect((await screen.findByRole('alert')).textContent).toContain(
      'window failed'
    )
  })

  it.each(['/settings', '/sessions/1/settings'])(
    'shows no navigation in the desktop settings window at %s',
    async (path) => {
      isTauri.mockReturnValue(true)
      renderShell(path)
      await screen.findByRole('heading')
      expect(
        screen.queryByRole('link', { name: 'WBook 工作台首页' })
      ).toBeNull()
      expect(screen.queryByRole('button', { name: '设置' })).toBeNull()
    }
  )

  it('navigates to the settings page in the browser', async () => {
    isTauri.mockReturnValue(false)
    const router = renderShell('/')
    fireEvent.click(await screen.findByRole('link', { name: '设置' }))
    await screen.findByRole('heading', { name: '/settings' })
    expect(router.state.location.pathname).toBe('/settings')
    expect(commands.openSettingsWindow).not.toHaveBeenCalled()
  })
})
