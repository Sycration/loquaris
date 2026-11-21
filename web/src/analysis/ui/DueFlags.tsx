// Spaced repetition flags (DESIGN.md "Spaced repetition flags"): an orange
// marker on cards, a line in the lesson footer and a side panel section.
import { useSyncExternalStore } from 'react'
import { useMeta, useStructure } from '../../doc/hooks'
import type { ScriptStructure } from '../../doc/projection'
import type { EditorServices } from '../../editor/context'
import type { CardDecorationProps, LessonFooterProps } from '../../editor/extensions'
import { useUi } from '../../editor/uiStore'
import { Spinner } from '../../ui/misc'
import type { ReviewFlag } from '../client.ts'
import { sessionFor } from '../session'
import { useSession } from './hooks'

const NONE: ReviewFlag[] = []

interface Grouped {
  byEntry: Map<string, ReviewFlag[]>
  byLesson: Map<string, ReviewFlag[]>
  /** Both kinds, by the lesson they are shown in. */
  inLesson: Map<string, ReviewFlag[]>
}

const grouped = new WeakMap<ReviewFlag[], Grouped>()

/** Flags indexed by card, lesson footer and lesson (computed once per answer). */
function group(flags: ReviewFlag[]): Grouped {
  let result = grouped.get(flags)
  if (result) return result
  result = { byEntry: new Map(), byLesson: new Map(), inLesson: new Map() }
  const push = (map: Map<string, ReviewFlag[]>, key: string, flag: ReviewFlag) => {
    const list = map.get(key)
    if (list) list.push(flag)
    else map.set(key, [flag])
  }
  for (const flag of flags) {
    push(result.inLesson, flag.kind.lessonId, flag)
    if (flag.kind.type === 'inLesson') push(result.byEntry, flag.kind.entryId, flag)
    else push(result.byLesson, flag.kind.lessonId, flag)
  }
  grouped.set(flags, result)
  return result
}

function useFlags(services: EditorServices): Grouped {
  const session = sessionFor(services.store)
  const flags = useSyncExternalStore(session.spaced.subscribe, session.spaced.getSnapshot)
  // Hide flags the moment the project switches them off, before the answer.
  const enabled = useMeta(services.store).spacedRepetition
  return group(enabled ? (flags ?? NONE) : NONE)
}

/** M:SS, rounded to the second. */
function clock(seconds: number): string {
  const total = Math.round(seconds)
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`
}

/** Where the word was last heard, for tooltips and the side panel. */
function lastHeard(flag: ReviewFlag, structure: ScriptStructure): string {
  const lessonIndex = structure.lessonIndex.get(flag.lastHeardLessonId)
  const lesson = lessonIndex === undefined ? undefined : structure.lessons[lessonIndex]
  const entry = (lesson?.entryIds.indexOf(flag.lastHeardEntryId) ?? -1) + 1
  if (flag.kind.type === 'inLesson') {
    return `last heard ${clock(flag.elapsedSeconds)} earlier${entry > 0 ? `, entry ${entry}` : ''}`
  }
  return `last heard in lesson ${(lessonIndex ?? 0) + 1}${entry > 0 ? `, entry ${entry}` : ''}`
}

/** An orange "⚑ rosa" on the card the review is due at. */
export function DueFlagMarker({ services, entry }: CardDecorationProps) {
  const flags = useFlags(services).byEntry.get(entry.id)
  const structure = useStructure(services.store)
  if (!flags) return null
  return (
    <>
      {flags.map((flag) => (
        <button
          key={`${flag.lemma.key}:${flag.lemma.extension}`}
          type="button"
          className="due-flag"
          data-due-flag
          title={`Due for review — ${lastHeard(flag, structure)}`}
          onClick={(event) => {
            event.stopPropagation()
            services.commands.reveal(flag.lastHeardEntryId)
          }}
        >
          ⚑ {flag.lemma.headword}
        </button>
      ))}
    </>
  )
}

/** "⚑ Not reviewed in this lesson: rosa, puella" in the lesson's footer bar. */
export function DueLessonFooter({ services, lessonId }: LessonFooterProps) {
  const flags = useFlags(services).byLesson.get(lessonId)
  if (!flags) return null
  const words = flags.map((flag) => flag.lemma.headword).join(', ')
  return (
    <span className="lesson-footer__due" data-due-footer title={`Due for review: ${words}`}>
      ⚑ Not reviewed in this lesson: {words}
    </span>
  )
}

const MATCH_ACTIVE = (state: { activeLesson: string | null }) => state.activeLesson

/** "Due for review": the active lesson's flags of both kinds. */
export function DueForReviewSection({ services }: { services: EditorServices }) {
  const session = useSession(services)
  const structure = useStructure(services.store)
  const active = useUi(services.ui, MATCH_ACTIVE)
  const groups = useFlags(services)
  const enabled = useMeta(services.store).spacedRepetition
  const loaded = useSyncExternalStore(session.spaced.subscribe, session.spaced.getSnapshot) !== null
  if (!enabled) return null
  const lessonId = active && structure.lessonIndex.has(active) ? active : (structure.lessons[0]?.id ?? null)
  if (!lessonId) return null
  const number = (structure.lessonIndex.get(lessonId) ?? 0) + 1
  const flags = groups.inLesson.get(lessonId) ?? NONE
  return (
    <section className="analysis" aria-label={`Due for review in lesson ${number}`} data-due-section>
      <h2 className="analysis__heading">Due for review</h2>
      {!loaded ? (
        session.status === 'failed' ? null : <Spinner label="Loading…" />
      ) : flags.length === 0 ? (
        <p className="analysis__empty">No words are due for review in this lesson.</p>
      ) : (
        <ul className="due-list">
          {flags.map((flag) => (
            <li key={`${flag.kind.type}:${flag.lemma.key}:${flag.lemma.extension}:${flag.kind.type === 'inLesson' ? flag.kind.entryId : ''}`} className="due-list__item">
              <button
                type="button"
                className="due-flag"
                title="Show where this word was last heard"
                onClick={() => services.commands.reveal(flag.lastHeardEntryId)}
              >
                ⚑ {flag.lemma.headword}
              </button>
              {flag.lemma.translation && <span className="analysis__translation">{flag.lemma.translation}</span>}
              <span className="due-list__why">
                {flag.kind.type === 'lesson' ? 'not in this lesson; ' : ''}
                {lastHeard(flag, structure)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
