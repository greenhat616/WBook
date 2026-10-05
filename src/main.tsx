import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { RouterProvider } from '@tanstack/react-router'
import { MotionConfig } from 'framer-motion'
import { router } from './router'
import './styles/global.css'

// Commands must not be replayed implicitly; callers decide when to retry.
const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false } }
})

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <MotionConfig reducedMotion="user">
        <RouterProvider router={router} />
      </MotionConfig>
    </QueryClientProvider>
  </React.StrictMode>
)
