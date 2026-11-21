import { useState } from 'react'
import { useLesson, useStructure } from '../../doc/hooks'
import type { EditorServices } from '../../editor/context'
import { useUi } from '../../editor/uiStore'
import { Checkbox } from '../../ui/Field'
import { Spinner } from '../../ui/misc'
import type { Lemma, QueryWord, SearchResult } from '../client.ts'
import { summaryLine } from '../search'
import { useAnalysisQuery, useSearchResult, useSearchState, useSession, useSessionVersion } from './hooks'

const MATCH_ACTIVE = (state: { activeLesson: string | null }) => state.activeLesson

/** Search box, options, summary and the analysis of the query. */
export function SearchSection({ services }: { services: EditorServices }) {
  const session = useSession(services)
  useSessionVersion(session)
  const state = useSearchState(session)
  const result = useSearchResult(session)
  const { search } = session
  const loading = session.status !== 'ready' && session.status !== 'failed'
  return (
    <section className="analysis" aria-label="Search">
      <h2 className="analysis__heading">Search</h2>
      <div className="analysis__box">
        <input
          className="input"
          type="search"
          value={state.query}
          placeholder="Search the project…"
          aria-label="Search the project"
          onChange={(event) => search.setQuery(event.target.value)}
        />
        {state.query && (
          <button type="button" className="analysis__clear" aria-label="Clear search" title="Clear search" onClick={() => search.clear()}>
            ✕
          </button>
        )}
      </div>
      <div className="analysis__options">
        <Checkbox label="Exact text" checked={state.exactText} onChange={(event) => search.setExactText(event.target.checked)} />
        <Checkbox label="Unmarked vowels are short" checked={state.unmarkedShort} onChange={(event) => search.setUnmarkedShort(event.target.checked)} />
      </div>
      {loading && <Spinner label="Loading the Latin analyzer…" />}
      {session.status === 'failed' && <p className="analysis__error">The Latin analyzer could not be loaded.</p>}
      {result && <p className="analysis__summary">{summaryLine(result)}</p>}
      {result && <AnalysisList result={result} unmarkedShort={state.unmarkedShort} onPick={(lemma) => search.pickLemma(lemma)} />}
    </section>
  )
}

function AnalysisList({ result, unmarkedShort, onPick }: { result: SearchResult; unmarkedShort: boolean; onPick: (lemma: Lemma) => void }) {
  if (result.analysis.length === 0) return null
  const single = result.analysis.length === 1
  return (
    <div className="analysis__words">
      <h3 className="analysis__subheading">Analysis</h3>
      {result.analysis.map((word, i) => (
        <WordAnalysis key={i} word={word} clickable={single} unmarkedShort={unmarkedShort} onPick={onPick} />
      ))}
    </div>
  )
}

function WordAnalysis({ word, clickable, unmarkedShort, onPick }: { word: QueryWord; clickable: boolean; unmarkedShort: boolean; onPick: (lemma: Lemma) => void }) {
  return (
    <div className="analysis__word">
      <div className="analysis__word-text latin">{word.text}</div>
      {word.ruledOut > 0 && (
        <div
          className="analysis__ruled-out"
          title={unmarkedShort ? 'Unmarked vowels are read as short, which rules these readings out.' : 'The vowel marks you wrote rule these readings out.'}
        >
          {word.ruledOut} other {word.ruledOut === 1 ? 'reading' : 'readings'} ruled out by vowel length
        </div>
      )}
      {word.lemmas.length === 0 && <div className="analysis__none">Not recognised as Latin.</div>}
      {word.lemmas.map((lemma) => (
        <div key={`${lemma.key}:${lemma.extension}`} className="analysis__lemma">
          <div>
            {clickable ? (
              <button type="button" className="analysis__lemma-link latin" title="Search every form of this lemma" onClick={() => onPick(lemma)}>
                {lemma.headword}
              </button>
            ) : (
              <span className="latin analysis__lemma-name">{lemma.headword}</span>
            )}
            {lemma.translation && <span className="analysis__translation"> {lemma.translation}</span>}
          </div>
          <ul className="analysis__readings">
            {lemma.readings.map((reading, i) => (
              <li key={i}>
                {reading.contextSelected && (
                  <span className="analysis__star" title="Most likely reading in context">
                    ★{' '}
                  </span>
                )}
                <span className="latin">{reading.form}</span>
                {reading.morphology ? ` — ${reading.morphology}` : ''}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </div>
  )
}

/** "New in lesson N": lemmas first used in the active lesson. */
export function NewWordsSection({ services }: { services: EditorServices }) {
  const session = useSession(services)
  const structure = useStructure(services.store)
  const active = useUi(services.ui, MATCH_ACTIVE)
  const lessonId = active && structure.lessonIndex.has(active) ? active : (structure.lessons[0]?.id ?? null)
  const lemmas = useAnalysisQuery<Lemma[]>(session, lessonId ?? '', (client) => client.newInLesson(lessonId!))
  if (!lessonId) return null
  const number = (structure.lessonIndex.get(lessonId) ?? 0) + 1
  return (
    <section className="analysis" aria-label={`New in lesson ${number}`}>
      <h2 className="analysis__heading">New in lesson {number}</h2>
      {lemmas === null ? (
        session.status === 'failed' ? null : <Spinner label="Loading…" />
      ) : lemmas.length === 0 ? (
        <p className="analysis__empty">No Latin words are introduced in this lesson.</p>
      ) : (
        <div className="analysis__chips">
          {lemmas.map((lemma) => (
            <button
              key={`${lemma.key}:${lemma.extension}`}
              type="button"
              className="chip chip--latin"
              title={lemma.translation ?? 'Search every form of this lemma'}
              onClick={() => session.search.pickLemma(lemma)}
            >
              {lemma.headword}
            </button>
          ))}
        </div>
      )}
    </section>
  )
}

/** "Contexts in lesson N": the active lesson's entries with hits. */
export function ContextsSection({ services }: { services: EditorServices }) {
  const session = useSession(services)
  const structure = useStructure(services.store)
  const result = useSearchResult(session)
  const state = useSearchState(session)
  const active = useUi(services.ui, MATCH_ACTIVE)
  const lessonId = active && structure.lessonIndex.has(active) ? active : (structure.lessons[0]?.id ?? null)
  if (!lessonId) return null
  const number = (structure.lessonIndex.get(lessonId) ?? 0) + 1
  const byEntry = new Map<string, { start: number; end: number }[]>()
  for (const hit of result?.hits ?? []) {
    if (services.store.getEntry(hit.entryId)?.lesson !== lessonId) continue
    const list = byEntry.get(hit.entryId) ?? []
    list.push(hit)
    byEntry.set(hit.entryId, list)
  }
  return (
    <section className="analysis" aria-label={`Contexts in lesson ${number}`}>
      <h2 className="analysis__heading">Contexts in lesson {number}</h2>
      {byEntry.size === 0 ? (
        <p className="analysis__empty">{state.query.trim() === '' ? 'Search to see each use in this lesson.' : 'Not used in this lesson.'}</p>
      ) : (
        <ul className="analysis__contexts">
          {[...byEntry].map(([entryId, hits]) => (
            <li key={entryId} className="analysis__context">
              <span className="analysis__context-text latin">{highlighted(services.store.getEntry(entryId)?.text ?? '', hits)}</span>
              <button type="button" className="analysis__goto" title="Select this entry in the script" aria-label="Select this entry in the script" onClick={() => services.commands.reveal(entryId)}>
                →
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}

function highlighted(text: string, hits: { start: number; end: number }[]) {
  const parts: React.ReactNode[] = []
  let at = 0
  for (const { start, end } of hits) {
    const s = Math.max(at, Math.min(start, text.length))
    const e = Math.max(s, Math.min(end, text.length))
    if (s > at) parts.push(text.slice(at, s))
    if (e > s) parts.push(<mark key={s} className="search-hit">{text.slice(s, e)}</mark>)
    at = e
  }
  if (at < text.length) parts.push(text.slice(at))
  return parts
}

const W = 280
const H = 120
const LEFT = 28
const MID = H / 2 - 6
const HALF = 40

/** "Uses per lesson": teacher above the midline, student below. */
export function UsesChartSection({ services }: { services: EditorServices }) {
  const session = useSession(services)
  const structure = useStructure(services.store)
  const result = useSearchResult(session)
  const active = useUi(services.ui, MATCH_ACTIVE)
  const [hover, setHover] = useState<number | null>(null)
  const lessons = structure.lessons
  if (lessons.length === 0) return null
  const counts = new Map(result?.perLesson.map((c) => [c.lessonId, c]))
  const rows = lessons.map((lesson) => counts.get(lesson.id) ?? { lessonId: lesson.id, teacher: 0, student: 0 })
  const max = Math.max(1, ...rows.map((r) => Math.max(r.teacher, r.student)))
  const colW = (W - LEFT) / lessons.length
  const hovered = hover !== null ? rows[hover] : null

  const go = (index: number) => {
    const lessonId = lessons[index].id
    const first = result?.hits.find((hit) => services.store.getEntry(hit.entryId)?.lesson === lessonId)
    if (first) services.commands.reveal(first.entryId)
    else services.commands.focusLesson(lessonId)
  }

  return (
    <section className="analysis" aria-label="Uses per lesson">
      <h2 className="analysis__heading">Uses per lesson</h2>
      <div className="uses-chart">
        <svg viewBox={`0 0 ${W} ${H}`} className="uses-chart__svg" role="img" aria-label="Uses per lesson">
          <line x1={LEFT} x2={W} y1={MID} y2={MID} className="uses-chart__axis" />
          <text x={LEFT - 4} y={MID - HALF + 4} textAnchor="end" className="uses-chart__label">{max}</text>
          <text x={LEFT - 4} y={MID + 3} textAnchor="end" className="uses-chart__label">0</text>
          <text x={LEFT - 4} y={MID + HALF + 4} textAnchor="end" className="uses-chart__label">{max}</text>
          <text x={LEFT} y={H - 2} className="uses-chart__label">Lesson 1</text>
          {lessons.length > 1 && <text x={W} y={H - 2} textAnchor="end" className="uses-chart__label">{lessons.length}</text>}
          {rows.map((row, i) => {
            const x = LEFT + i * colW
            const shaded = lessons[i].id === active || i === hover
            const bar = Math.max(1, colW * 0.6)
            return (
              <g key={row.lessonId}>
                {shaded && <rect x={x} y={MID - HALF - 6} width={colW} height={HALF * 2 + 12} className="uses-chart__shade" />}
                {row.teacher > 0 && <rect x={x + (colW - bar) / 2} y={MID - (row.teacher / max) * HALF} width={bar} height={(row.teacher / max) * HALF} fill="#3987e5" />}
                {row.student > 0 && <rect x={x + (colW - bar) / 2} y={MID} width={bar} height={(row.student / max) * HALF} fill="#d95926" />}
                <rect
                  x={x}
                  y={0}
                  width={colW}
                  height={H - 12}
                  fill="transparent"
                  className="uses-chart__hit"
                  onMouseEnter={() => setHover(i)}
                  onMouseLeave={() => setHover(null)}
                  onClick={() => go(i)}
                />
              </g>
            )
          })}
        </svg>
        {hover !== null && hovered && <ChartTooltip services={services} lessonId={lessons[hover].id} index={hover} teacher={hovered.teacher} student={hovered.student} left={((LEFT + (hover + 0.5) * colW) / W) * 100} />}
      </div>
    </section>
  )
}

function ChartTooltip({ services, lessonId, index, teacher, student, left }: { services: EditorServices; lessonId: string; index: number; teacher: number; student: number; left: number }) {
  const lesson = useLesson(services.store, lessonId)
  return (
    <div className="uses-chart__tooltip" style={{ left: `${left}%` }} role="tooltip">
      <strong>
        Lesson {index + 1}
        {lesson?.name ? `: ${lesson.name}` : ''}
      </strong>
      <div>
        Teacher {teacher} · Student {student}
      </div>
      <div className="uses-chart__hint">Click to go to the first use</div>
    </div>
  )
}
