import { useState, type FormEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import SearchIcon from '~icons/material-symbols/image-search-rounded'
import OpenIcon from '~icons/material-symbols/open-in-new-rounded'
import { queries, type CoverSearchSettings } from '@/bindings'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useSettingsUpdates } from '@/features/settings/use-settings-updates'

const isAddress = (text: string) => /^https?:\/\//i.test(text.trim())
const fill = (text: string, key: string, value: string) =>
  text.split(key).join(value)

export function searchText(
  template: string,
  title: string,
  author: string | null
): string {
  return fill(fill(template, '{title}', title), '{author}', author ?? '')
    .replace(/\s+/g, ' ')
    .trim()
}

/** The search row with the global sources, kept current across windows. */
export function GlobalCoverSearch(props: Omit<Props, 'search'>) {
  const stored = useQuery(queries.getSettings())
  useSettingsUpdates()
  const search = stored.data?.settings.cover_search
  return search ? (
    // A changed template or title restarts the prefilled search text.
    <CoverSearch
      key={`${search.query}\u0000${props.title}\u0000${props.author}`}
      search={search}
      {...props}
    />
  ) : null
}

type Props = {
  search: CoverSearchSettings
  title: string
  author: string | null
  disabled: boolean
  onOpen: (url: string) => void
}

/** Searches a configured image source, or opens an address typed instead. */
export function CoverSearch({
  search,
  title,
  author,
  disabled,
  onOpen
}: Props) {
  const [engine, setEngine] = useState(0)
  const [text, setText] = useState(() =>
    searchText(search.query, title, author)
  )
  const address = isAddress(text)
  const source = search.engines[engine] ?? search.engines[0]
  const ready = text.trim() !== '' && (address || !!source)

  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!ready) return
    onOpen(
      address
        ? text.trim()
        : fill(source.url, '{query}', encodeURIComponent(text.trim()))
    )
  }

  return (
    <form
      onSubmit={submit}
      aria-label="网络搜索封面"
      className="flex flex-wrap items-center gap-1.5"
    >
      <select
        aria-label="图片源"
        value={engine}
        disabled={disabled || address || search.engines.length === 0}
        onChange={(event) => setEngine(Number(event.target.value))}
        className="h-8 rounded-lg border border-input bg-background px-2 text-xs disabled:opacity-50"
      >
        {search.engines.map((item, index) => (
          <option key={`${index}-${item.name}`} value={index}>
            {item.name}
          </option>
        ))}
      </select>
      <Input
        aria-label="搜索内容或网址"
        value={text}
        onChange={(event) => setText(event.target.value)}
        disabled={disabled}
        placeholder="搜索内容，或以 http(s):// 开头的网址"
        autoComplete="off"
        spellCheck={false}
        className="h-8 min-w-[12rem] flex-1 rounded-lg px-2.5"
      />
      <Button
        type="submit"
        size="sm"
        variant="outline"
        disabled={disabled || !ready}
      >
        {address ? (
          <OpenIcon aria-hidden="true" />
        ) : (
          <SearchIcon aria-hidden="true" />
        )}
        {address ? '打开网址' : '搜索图片'}
      </Button>
    </form>
  )
}
