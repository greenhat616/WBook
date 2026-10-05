import { Link } from '@tanstack/react-router'
import { motion, useReducedMotion } from 'framer-motion'
import { BookOpenText, Library, Leaf } from 'lucide-react'
import { useRef, type ReactNode } from 'react'

export function AppShell({ children }: { children: ReactNode }) {
  const reducedMotion = useReducedMotion()
  const mainRef = useRef<HTMLElement>(null)

  return (
    <div className="mx-auto min-h-dvh max-w-[1600px] lg:grid lg:grid-cols-[200px_minmax(0,1fr)]">
      <a
        href="#main-content"
        onClick={(event) => {
          // Native hash navigation would replace the desktop router's location.
          event.preventDefault()
          mainRef.current?.focus()
        }}
        className="sr-only fixed left-4 top-4 z-50 rounded-full bg-primary px-5 py-3 font-medium text-primary-foreground focus:not-sr-only"
      >
        跳到主要内容
      </a>

      <aside className="flex items-center justify-between gap-4 border-b border-border/60 px-5 py-4 sm:px-8 lg:sticky lg:top-0 lg:h-dvh lg:flex-col lg:items-stretch lg:justify-start lg:border-b-0 lg:border-r lg:px-5 lg:py-8">
        <Link
          to="/"
          aria-label="WBook 工作台首页"
          className="flex w-fit items-center gap-3 rounded-xl lg:mb-12 lg:px-2"
        >
          <span className="flex size-11 shrink-0 items-center justify-center rounded-2xl rounded-br-md bg-primary text-primary-foreground">
            <BookOpenText
              className="size-6"
              strokeWidth={1.7}
              aria-hidden="true"
            />
          </span>
          <span>
            <span className="block text-xl font-bold tracking-tight">
              WBook
            </span>
            <span className="hidden text-[10px] font-medium tracking-[0.16em] text-muted-foreground sm:block">
              LOCAL BOOK STUDIO
            </span>
          </span>
        </Link>

        <nav aria-label="主导航">
          <Link
            to="/"
            activeOptions={{ exact: true }}
            className="flex items-center gap-3 rounded-full px-4 py-3 text-sm font-semibold text-muted-foreground transition-colors hover:bg-secondary hover:text-secondary-foreground lg:rounded-2xl"
            activeProps={{
              className: 'bg-secondary text-secondary-foreground'
            }}
          >
            <Library className="size-5" aria-hidden="true" />
            工作台
          </Link>
        </nav>

        <div className="mt-auto hidden px-3 pb-1 lg:block">
          <Leaf
            className="mb-3 size-5 text-primary/70"
            strokeWidth={1.5}
            aria-hidden="true"
          />
          <p className="text-sm font-medium">把文字，整理成书。</p>
          <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
            文件在本机处理，
            <br />
            专注当下的创作。
          </p>
        </div>
      </aside>

      <motion.main
        ref={mainRef}
        id="main-content"
        tabIndex={-1}
        initial={reducedMotion ? false : { opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: reducedMotion ? 0 : 0.25 }}
        className="mx-auto w-full min-w-0 max-w-[1320px] px-4 py-7 focus:outline-none sm:px-8 sm:py-10 xl:px-10"
      >
        {children}
      </motion.main>
    </div>
  )
}
