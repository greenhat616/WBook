// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { TocSettings } from '../../src/bindings'
import { ParserPanel } from '../../src/features/sessions/components/parser-panel'

const toc: TocSettings = {
  mode: 'VBook',
  chapter_marks: ['章', '回', '节', '集'],
  volume_marks: ['部', '卷'],
  max_title_len: 25,
  volume_split: 'Titles',
  chapters_per_volume: 50,
  parts: 10
}
const other: TocSettings = { ...toc, max_title_len: 40 }

afterEach(cleanup)

function renderPanel(settings: TocSettings) {
  const onParse = vi.fn()
  const view = render(
    <ParserPanel
      settings={settings}
      disabled={false}
      ready
      stale={false}
      onParse={onParse}
    />
  )
  const update = (next: TocSettings) =>
    view.rerender(
      <ParserPanel
        settings={next}
        disabled={false}
        ready
        stale={false}
        onParse={onParse}
      />
    )
  return { onParse, update }
}

const mode = (name: string) =>
  screen.getByRole('radio', { name }).getAttribute('aria-checked')

describe('parser panel', () => {
  it('accepts no automatic volumes, which fixed-size volumes cannot use', () => {
    const { onParse } = renderPanel({ ...toc, chapters_per_volume: 0 })
    fireEvent.click(screen.getByRole('button', { name: '试解析' }))
    expect(onParse).toHaveBeenLastCalledWith({ ...toc, chapters_per_volume: 0 })
    fireEvent.change(screen.getByLabelText('分卷方式'), {
      target: { value: 'Forced' }
    })
    expect((screen.getByLabelText(/每卷章数/) as HTMLInputElement).value).toBe(
      '50'
    )
  })

  it('follows rules saved elsewhere while it has no edits', () => {
    const { update } = renderPanel(toc)
    update({ ...other, mode: 'Chapters' })
    expect(mode('仅章节')).toBe('true')
    expect(screen.queryByText(/已在其他窗口更新/)).toBeNull()
  })

  it('keeps edits but refuses to parse over rules saved elsewhere', () => {
    const { onParse, update } = renderPanel(toc)
    fireEvent.click(screen.getByRole('radio', { name: '仅章节' }))
    update(other)

    expect(screen.getByRole('alert').textContent).toContain('已在其他窗口更新')
    expect(mode('仅章节')).toBe('true')
    const parse = screen.getByRole('button', { name: '试解析' })
    expect(parse).toHaveProperty('disabled', true)
    fireEvent.submit(parse.closest('form')!)
    expect(onParse).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: '载入最新' }))
    expect(mode('仅章节')).toBe('false')
    fireEvent.click(screen.getByRole('button', { name: '试解析' }))
    expect(onParse).toHaveBeenCalledWith(other)
  })

  it('treats its own rules coming back as saved', () => {
    const { update } = renderPanel(toc)
    fireEvent.click(screen.getByRole('radio', { name: '仅章节' }))
    update({ ...toc, mode: 'Chapters' })
    expect(screen.queryByText(/已在其他窗口更新/)).toBeNull()
    expect(screen.getByRole('button', { name: '撤销修改' })).toBeTruthy()
  })
})
