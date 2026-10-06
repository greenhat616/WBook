import { Link } from '@tanstack/react-router'
import ArrowLeftIcon from '~icons/material-symbols/arrow-back-rounded'
import { commands } from '@/bindings'
import { Button } from '@/components/ui/button'
import { useSession } from '@/features/sessions/use-session'
import { SettingsScreen } from '@/features/settings/settings-screen'

export function SessionSettingsPage({ sessionId }: { sessionId: number }) {
  const session = useSession(sessionId)
  const { snapshot, settings, warnings, error } = session
  const name = snapshot
    ? snapshot.source.split(/[\\/]/).pop() || snapshot.source
    : ''
  const open =
    snapshot?.lifecycle === 'Open' && !!snapshot.workspace_status.Available

  return (
    <div className="mx-auto w-full max-w-3xl space-y-4 px-4 pb-6 pt-4 sm:px-6">
      <header className="flex items-start gap-2">
        <Button asChild variant="ghost" size="icon-sm" className="shrink-0">
          <Link
            to="/sessions/$sessionId"
            params={{ sessionId: String(sessionId) }}
            aria-label="返回工作区"
            title="返回工作区"
          >
            <ArrowLeftIcon aria-hidden="true" />
          </Link>
        </Button>
        <div className="min-w-0 space-y-1">
          <h1 className="truncate text-2xl font-semibold tracking-tight">
            本书设置{name && ` · ${name}`}
          </h1>
          <p className="text-sm text-muted-foreground">
            只影响这本书。新的解析规则在工作区试解析后生效；渲染设置变更后需要重新生成预览。
          </p>
        </div>
      </header>

      {(warnings.length > 0 || (error && !settings)) && (
        <div
          role="alert"
          className="space-y-1 break-words rounded-2xl bg-destructive/10 p-4 text-sm text-destructive"
        >
          {error && !settings && <p>{error}</p>}
          {warnings.map((warning, index) => (
            <p key={index}>
              清理失败：{warning.path}（{warning.message}）
            </p>
          ))}
        </div>
      )}

      {settings ? (
        <SettingsScreen
          saved={settings}
          disabled={!open}
          onSave={session.saveSettings}
          restore={{
            label: '恢复为全局设置',
            load: async () => (await commands.getSettings()).settings
          }}
        />
      ) : snapshot && !open ? (
        <p role="status" className="px-1 text-sm text-muted-foreground">
          会话已关闭或不可用，无法修改设置。
        </p>
      ) : (
        <p role="status" className="px-1 text-sm text-muted-foreground">
          正在读取本书设置…
        </p>
      )}
    </div>
  )
}
