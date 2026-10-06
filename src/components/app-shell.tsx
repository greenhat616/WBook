import { Link } from '@tanstack/react-router'
import { motion, useReducedMotion } from 'framer-motion'
import { M3eAppBar } from '@m3e/react/app-bar'
import { M3eIconButton } from '@m3e/react/icon-button'
import { useRef, type ReactNode } from 'react'
import BookIcon from '~icons/material-symbols/menu-book-outline-rounded'
import NotificationsIcon from '~icons/material-symbols/notifications-outline-rounded'
import SettingsIcon from '~icons/material-symbols/settings-outline-rounded'

// Notifications and settings have no screens yet; keep them visible but inert.
const pendingActions = [
  { label: '通知', Icon: NotificationsIcon },
  { label: '设置', Icon: SettingsIcon }
]

export function AppShell({ children }: { children: ReactNode }) {
  const reducedMotion = useReducedMotion()
  const mainRef = useRef<HTMLElement>(null)

  return (
    <div className="flex h-dvh flex-col">
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

      <M3eAppBar
        htmlFor="main-content"
        className="[--m3e-app-bar-container-color:var(--md-sys-color-surface)] [--m3e-app-bar-padding-left:1rem] [--m3e-app-bar-padding-right:1rem] [--m3e-app-bar-small-container-height:3.25rem]"
      >
        <Link
          slot="leading"
          to="/"
          aria-label="WBook 工作台首页"
          className="flex size-10 items-center justify-center rounded-xl bg-primary-container text-primary-on-container transition-[border-radius] duration-200 hover:rounded-2xl"
        >
          <BookIcon className="size-6" aria-hidden="true" />
        </Link>
        <span slot="title" className="font-semibold tracking-tight">
          WBook
        </span>
        {pendingActions.map(({ label, Icon }) => (
          <M3eIconButton
            key={label}
            slot="trailing"
            aria-label={`${label}（即将推出）`}
            title={`${label}（即将推出）`}
            disabledInteractive
          >
            <Icon className="size-6" aria-hidden="true" />
          </M3eIconButton>
        ))}
      </M3eAppBar>

      <motion.main
        ref={mainRef}
        id="main-content"
        tabIndex={-1}
        initial={reducedMotion ? false : { opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: reducedMotion ? 0 : 0.25 }}
        className="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto focus:outline-none"
      >
        {children}
      </motion.main>
    </div>
  )
}
