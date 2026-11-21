// Text with remote selections, remote carets and search highlights, used both
// for a card's text when viewing and as the mirror behind a textarea when
// editing (identical metrics, so remote carets line up with the textarea).
import { memo, type CSSProperties, type ReactNode } from 'react'
import type { TextRange } from './extensions'

export interface RemoteMark {
  clientId: number
  name: string
  color: string
  /** Selection [start, end) — equal for a bare caret. */
  start: number
  end: number
  /** Where the caret is drawn (the selection head). */
  head: number
}

export interface DecoratedTextProps {
  text: string
  remote?: readonly RemoteMark[]
  highlights?: readonly TextRange[]
  placeholder?: string
}

interface Segment {
  start: number
  end: number
  selections: RemoteMark[]
  highlighted: boolean
}

export const DecoratedText = memo(function DecoratedText({ text, remote = [], highlights = [], placeholder }: DecoratedTextProps) {
  if (text.length === 0) {
    return (
      <>
        {placeholder && <span className="placeholder">{placeholder}</span>}
        {remote.map((mark) => (
          <Caret key={mark.clientId} mark={mark} />
        ))}
        {'​'}
      </>
    )
  }
  if (remote.length === 0 && highlights.length === 0) {
    // A trailing newline needs a character after it to take up a line.
    return <>{text.endsWith('\n') ? `${text}​` : text}</>
  }
  const length = text.length
  const clamp = (n: number) => Math.max(0, Math.min(length, n))
  const bounds = new Set<number>([0, length])
  for (const mark of remote) {
    bounds.add(clamp(mark.start))
    bounds.add(clamp(mark.end))
    bounds.add(clamp(mark.head))
  }
  for (const range of highlights) {
    bounds.add(clamp(range.start))
    bounds.add(clamp(range.end))
  }
  const points = Array.from(bounds).sort((a, b) => a - b)
  const segments: Segment[] = []
  for (let i = 0; i < points.length - 1; i++) {
    const start = points[i]
    const end = points[i + 1]
    segments.push({
      start,
      end,
      selections: remote.filter((mark) => mark.start <= start && mark.end >= end && mark.start !== mark.end),
      highlighted: highlights.some((range) => range.start <= start && range.end >= end),
    })
  }
  const caretsAt = (index: number) => remote.filter((mark) => clamp(mark.head) === index)
  const nodes: ReactNode[] = []
  for (const segment of segments) {
    for (const mark of caretsAt(segment.start)) nodes.push(<Caret key={`c${mark.clientId}`} mark={mark} />)
    const content = text.slice(segment.start, segment.end)
    const style: CSSProperties | undefined = segment.selections.length
      ? { backgroundColor: `color-mix(in srgb, ${segment.selections[segment.selections.length - 1].color} 32%, transparent)` }
      : undefined
    nodes.push(
      <span key={`s${segment.start}`} className={segment.highlighted ? 'search-hit' : undefined} style={style}>
        {content}
      </span>,
    )
  }
  for (const mark of caretsAt(length)) nodes.push(<Caret key={`c${mark.clientId}`} mark={mark} />)
  if (text.endsWith('\n')) nodes.push('​')
  return <>{nodes}</>
})

function Caret({ mark }: { mark: RemoteMark }) {
  return (
    <span className="remote-caret" style={{ '--caret-color': mark.color } as CSSProperties} aria-hidden>
      <span className="remote-caret__flag">{mark.name}</span>
    </span>
  )
}
