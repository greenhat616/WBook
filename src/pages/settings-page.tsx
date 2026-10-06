import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { commands, mutations, queries } from '@/bindings'
import { Button } from '@/components/ui/button'
import { errorMessage } from '@/features/sessions/api'
import { SettingsScreen } from '@/features/settings/settings-screen'

export function SettingsPage() {
  const queryClient = useQueryClient()
  const stored = useQuery(queries.getSettings())
  const saving = useMutation(mutations.saveSettings())

  return (
    <div className="mx-auto w-full max-w-3xl space-y-4 px-4 pb-6 pt-4 sm:px-6">
      <header className="space-y-1 px-1">
        <h1 className="text-2xl font-semibold tracking-tight">设置</h1>
        <p className="text-sm text-muted-foreground">
          新建的工作会话会复制这里的设置；已经打开的会话保留各自的设置。
        </p>
      </header>

      {stored.data ? (
        <SettingsScreen
          saved={stored.data.settings}
          onSave={async (settings) => {
            const next = await saving.mutateAsync({ settings })
            queryClient.setQueryData(queries.getSettings().queryKey, next)
          }}
          restore={{ label: '恢复默认', load: commands.defaultSettings }}
        >
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
