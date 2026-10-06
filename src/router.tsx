import {
  createHashHistory,
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet
} from '@tanstack/react-router'
import { AppShell } from '@/components/app-shell'
import { Button } from '@/components/ui/button'
import { useWindowReady } from '@/hooks/use-window-ready'
import { HomePage } from '@/pages/home-page'
import { SessionPage } from '@/pages/session-page'

function MissingPage() {
  return (
    <section className="mx-auto max-w-lg py-20 text-center">
      <p className="mb-3 text-sm text-muted-foreground">找不到这个工作区</p>
      <h1 className="mb-6 text-3xl font-semibold">
        回到工作台，继续整理文字。
      </h1>
      <Button asChild>
        <Link to="/">返回工作台</Link>
      </Button>
    </section>
  )
}

function RootLayout() {
  useWindowReady()
  return (
    <AppShell>
      <Outlet />
    </AppShell>
  )
}

const rootRoute = createRootRoute({
  component: RootLayout,
  notFoundComponent: MissingPage,
  errorComponent: ({ error, reset }) => (
    <section role="alert" className="mx-auto max-w-lg space-y-4 py-16">
      <h1 className="text-2xl font-semibold">页面暂时无法显示</h1>
      <p className="break-words text-muted-foreground">
        {error instanceof Error ? error.message : String(error)}
      </p>
      <Button onClick={reset}>重试</Button>
      <Button asChild variant="ghost">
        <Link to="/">返回工作台</Link>
      </Button>
    </section>
  )
})

const homeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: HomePage
})

const sessionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/sessions/$sessionId',
  component: SessionRoute
})

function SessionRoute() {
  const { sessionId } = sessionRoute.useParams()
  const id = Number(sessionId)
  return /^\d+$/.test(sessionId) && Number.isSafeInteger(id) && id > 0 ? (
    <SessionPage key={id} sessionId={id} />
  ) : (
    <MissingPage />
  )
}

export const router = createRouter({
  routeTree: rootRoute.addChildren([homeRoute, sessionRoute]),
  history: createHashHistory(),
  scrollRestoration: true
})

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}
