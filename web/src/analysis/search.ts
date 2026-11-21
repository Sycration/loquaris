// The search state of an editor (query, options, picked lemma) and its
// latest result. Lives in the session so card highlights, the side panel and
// "select text to search" all share it.
import type { Answer, LemmaRef, SearchResult } from './client.ts'
import type { TextRange } from '../editor/extensions'
import type { AnalysisSession } from './session'

export interface SearchState {
  query: string
  exactText: boolean
  unmarkedShort: boolean
  /** An explicitly picked lemma (takes precedence over exactText). */
  lemma: LemmaRef | null
}

type Listener = () => void

const NONE: readonly TextRange[] = []
const INITIAL: SearchState = { query: '', exactText: false, unmarkedShort: false, lemma: null }

export class SearchController {
  private state: SearchState = INITIAL
  private result: SearchResult | null = null
  private ranges = new Map<string, readonly TextRange[]>()
  private listeners = new Set<Listener>()
  private unsubSession: (() => void) | null = null
  private sequence = 0

  private readonly session: AnalysisSession

  constructor(session: AnalysisSession) {
    this.session = session
  }

  getState = (): SearchState => this.state
  getResult = (): SearchResult | null => this.result
  /** Stable per entry until its hits change. */
  rangesFor(entryId: string): readonly TextRange[] {
    return this.ranges.get(entryId) ?? NONE
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    if (!this.unsubSession) {
      this.unsubSession = this.session.subscribe(() => this.run())
      this.run()
    }
    return () => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0) {
        this.unsubSession?.()
        this.unsubSession = null
      }
    }
  }

  private emit(): void {
    for (const listener of [...this.listeners]) listener()
  }

  /** Editing the query drops an explicit lemma. */
  setQuery(query: string): void {
    this.update({ ...this.state, query, lemma: null })
  }

  setExactText(exactText: boolean): void {
    // Exact text forces text mode, so it overrides a picked lemma.
    this.update({ ...this.state, exactText, lemma: exactText ? null : this.state.lemma })
  }

  setUnmarkedShort(unmarkedShort: boolean): void {
    this.update({ ...this.state, unmarkedShort })
  }

  /** Search every form of a lemma; turns exact text off. */
  pickLemma(lemma: { key: string; extension: boolean; headword: string }): void {
    this.update({
      ...this.state,
      query: lemma.headword,
      exactText: false,
      lemma: { key: lemma.key, extension: lemma.extension },
    })
  }

  clear(): void {
    this.update({ ...this.state, query: '', lemma: null })
  }

  reset(): void {
    this.sequence++
    this.result = null
    this.ranges = new Map()
  }

  private update(state: SearchState): void {
    this.state = state
    this.emit()
    this.run()
  }

  private run(): void {
    const client = this.session.api
    const { query, exactText, unmarkedShort, lemma } = this.state
    if (query.trim() === '') {
      if (this.result) {
        this.sequence++
        this.setResult(null)
      }
      return
    }
    if (!client || this.session.status !== 'ready') return
    const sequence = ++this.sequence
    client.search(query, { exactText, unmarkedShort, ...(lemma ? { lemma } : {}) }).then(
      (answer: Answer<SearchResult>) => {
        if (sequence !== this.sequence || this.session.api !== client || !client.isCurrent(answer)) return
        this.setResult(answer.value)
      },
      () => {},
    )
  }

  private setResult(result: SearchResult | null): void {
    const next = new Map<string, TextRange[]>()
    for (const hit of result?.hits ?? []) {
      const list = next.get(hit.entryId) ?? []
      list.push({ start: hit.start, end: hit.end })
      next.set(hit.entryId, list)
    }
    // Keep the previous array where nothing changed so cards don't re-render.
    for (const [id, list] of next) {
      const old = this.ranges.get(id)
      if (old && old.length === list.length && old.every((r, i) => r.start === list[i].start && r.end === list[i].end)) {
        next.set(id, old as TextRange[])
      }
    }
    this.result = result
    this.ranges = next
    this.emit()
  }
}

/** "N uses of the lemma X in M lessons" (FEATURES §15). */
export function summaryLine(result: SearchResult): string {
  const n = result.hits.length
  const uses = `${n} ${n === 1 ? 'use' : 'uses'}`
  const subject = result.mode === 'lemma' && result.lemma ? `of the lemma ${result.lemma.headword}` : 'of this text'
  const m = result.lessonsWithHits
  return `${uses} ${subject}${m > 0 ? ` in ${m} ${m === 1 ? 'lesson' : 'lessons'}` : ''}`
}
