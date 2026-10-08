import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCanGoBack, useRouter } from '@tanstack/react-router'
import ArrowLeftIcon from '~icons/material-symbols/arrow-back-rounded'
import { commands, mutations, queries } from '@/bindings'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/features/sessions/api'
import { SettingsScreen } from '@/features/settings/settings-screen'
import { useSettingsUpdates } from '@/features/settings/use-settings-updates'

export function SettingsPage() {
  const queryClient = useQueryClient()
  const stored = useQuery(queries.getSettings())
  const saving = useMutation(mutations.saveSettings())
  const updates = useSettingsUpdates()
  const router = useRouter()
  const canGoBack = useCanGoBack()

  // Session windows open settings from their own page, so going home would
  // strand them on the main window's screen.
  function close() {
    if (canGoBack) router.history.back()
    else void router.navigate({ to: '/' })
  }

  return (
    <div className="mx-auto w-full max-w-3xl space-y-4 px-4 pb-6 pt-4 sm:px-6">
      <header className="flex items-start gap-2">
        <Button
          variant="ghost"
          size="icon-sm"
          className="shrink-0"
          aria-label="关闭设置"
          title="关闭设置"
          onClick={close}
        >
          <ArrowLeftIcon aria-hidden="true" />
        </Button>
        <div className="min-w-0 space-y-1">
          <h1 className="text-2xl font-semibold tracking-tight">设置</h1>
          <p className="text-sm text-muted-foreground">
            新建的工作会话会复制这里的设置；已经打开的会话保留各自的设置。
          </p>
        </div>
      </header>

      {stored.data ? (
        <SettingsScreen
          saved={stored.data.settings}
          onSave={async (settings) => {
            const { queryKey } = queries.getSettings()
            // The form refuses to save once it is outdated, so the latest
            // known revision is the one its edits started from.
            const expected = queryClient.getQueryData(queryKey)!.revision
            const next = await saving.mutateAsync({ expected, settings })
            queryClient.setQueryData(queryKey, (cached) =>
              cached && cached.revision > next.revision ? cached : next
            )
          }}
          restore={{ label: '恢复默认', load: commands.defaultSettings }}
        >
          {updates.error && (
            <div
              role="status"
              className="flex flex-wrap items-center gap-2 rounded-2xl bg-surface-container-high p-3 text-sm"
            >
              <span className="min-w-0 flex-1 break-words text-muted-foreground">
                无法接收其他窗口的设置更新：{updates.error}
              </span>
              <Button size="sm" variant="ghost" onClick={updates.reconnect}>
                重新连接
              </Button>
            </div>
          )}
          {stored.data.problem && (
            <p
              role="alert"
              className="break-words rounded-2xl bg-destructive/10 p-4 text-sm text-destructive"
            >
              设置文件无法使用，当前显示默认设置；保存后会覆盖它。
              <br />
              {stored.data.problem}
            </p>
          )}
        </SettingsScreen>
      ) : stored.error ? (
        <div role="alert" className="space-y-3 rounded-2xl bg-card p-6">
          <p className="text-sm text-destructive">
            {errorMessage(stored.error)}
          </p>
          <Button size="sm" onClick={() => void stored.refetch()}>
            重试
          </Button>
        </div>
      ) : (
        <p role="status" className="px-1 text-sm text-muted-foreground">
          正在读取设置…
        </p>
      )}
    </div>
  )
}
