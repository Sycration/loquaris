// A reactive, fine-grained projection of the project Y.Doc for React.
//
// The store keeps immutable snapshots — the ordered structure, one per
// lesson, one per entry, and the meta — and replaces only those a
// transaction touched. Components subscribe to exactly what they show via
// useSyncExternalStore (see ./hooks.ts), so typing in one card re-renders
// that card only, and a move re-renders the lessons whose entry lists changed.
import * as Y from 'yjs'
import { readStructure, type Structure, type StructureLesson } from './ops'
import {
  entryAudio,
  entryLanguage,
  entryLemmaChoices,
  entryLesson,
  entryOrder,
  entryParticipant,
  entryTextString,
  entryTrim,
  lessonNameString,
  lessonOrder,
  readMeta,
  roots,
  type AudioRef,
  type Language,
  type LemmaChoice,
  type Meta,
  type Participant,
  type Trim,
  type YEntry,
  type YLesson,
} from './schema'
import { lessonRecordingSummary } from './summary'

export interface EntrySnapshot {
  id: string
  lesson: string
  order: string
  participant: Participant
  language: Language
  text: string
  audio: AudioRef | null
  trim: Trim | null
  lemmas: Readonly<Record<string, LemmaChoice>>
}

export interface LessonSnapshot {
  id: string
  name: string
  order: string
}

export interface EntryPosition {
  /** Index of the lesson in script order. */
  lesson: number
  /** Index within the lesson. */
  index: number
  /** Index in the whole script. */
  flat: number
}

export interface ScriptStructure extends Structure {
  lessonIndex: ReadonlyMap<string, number>
  position: ReadonlyMap<string, EntryPosition>
}

type Listener = () => void

const STRUCTURAL_ENTRY_KEYS = new Set(['lesson', 'order'])
const RECORDING_ENTRY_KEYS = new Set(['participant', 'audio', 'trim'])
const RECORDING_META_KEYS = new Set(['gapSameMs', 'gapDifferentMs'])

function entrySnapshot(id: string, entry: YEntry): EntrySnapshot {
  return {
    id,
    lesson: entryLesson(entry),
    order: entryOrder(entry),
    participant: entryParticipant(entry),
    language: entryLanguage(entry),
    text: entryTextString(entry),
    audio: entryAudio(entry),
    trim: entryTrim(entry),
    lemmas: entryLemmaChoices(entry),
  }
}

function lessonSnapshot(id: string, lesson: YLesson): LessonSnapshot {
  return { id, name: lessonNameString(lesson), order: lessonOrder(lesson) }
}

function sameIds(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((id, i) => id === b[i])
}

export class DocStore {
  readonly doc: Y.Doc
  private structure: ScriptStructure
  private entries = new Map<string, EntrySnapshot>()
  private lessons = new Map<string, LessonSnapshot>()
  private meta: Meta
  private recordingsVersion = 0
  private summaries = new Map<string, { version: number; value: string }>()

  private structureListeners = new Set<Listener>()
  private metaListeners = new Set<Listener>()
  private recordingListeners = new Set<Listener>()
  private entryListeners = new Map<string, Set<Listener>>()
  private lessonListeners = new Map<string, Set<Listener>>()

  // Dirty state collected by the observers of one transaction.
  private dirtyStructure = false
  private dirtyMeta = false
  private dirtyRecordings = false
  private dirtyEntries = new Set<string>()
  private dirtyLessons = new Set<string>()

  /** Called (in a microtask) when entries of deleted lessons are found.
   * The editor removes them if it may write. */
  onOrphans: ((ids: string[]) => void) | null = null

  private readonly disposers: (() => void)[] = []

  constructor(doc: Y.Doc) {
    this.doc = doc
    const { meta, lessons, entries } = roots(doc)
    entries.forEach((entry, id) => this.entries.set(id, entrySnapshot(id, entry)))
    lessons.forEach((lesson, id) => this.lessons.set(id, lessonSnapshot(id, lesson)))
    this.meta = readMeta(meta)
    this.structure = this.buildStructure(null)

    const onEntries = (events: Y.YEvent<Y.AbstractType<unknown>>[]) => {
      for (const event of events) {
        if (event.target === entries) {
          this.dirtyStructure = true
          this.dirtyRecordings = true
          for (const key of event.changes.keys.keys()) this.dirtyEntries.add(key)
          continue
        }
        const id = event.path[0] as string
        this.dirtyEntries.add(id)
        if (event.path.length === 1 && event instanceof Y.YMapEvent) {
          for (const key of event.keysChanged) {
            if (STRUCTURAL_ENTRY_KEYS.has(key)) {
              this.dirtyStructure = true
              this.dirtyRecordings = true
            }
            if (RECORDING_ENTRY_KEYS.has(key)) this.dirtyRecordings = true
          }
        }
      }
    }
    const onLessons = (events: Y.YEvent<Y.AbstractType<unknown>>[]) => {
      for (const event of events) {
        if (event.target === lessons) {
          this.dirtyStructure = true
          for (const key of event.changes.keys.keys()) this.dirtyLessons.add(key)
          continue
        }
        const id = event.path[0] as string
        this.dirtyLessons.add(id)
        if (event.path.length === 1 && event instanceof Y.YMapEvent && event.keysChanged.has('order')) {
          this.dirtyStructure = true
        }
      }
    }
    const onMeta = (event: Y.YMapEvent<unknown>) => {
      this.dirtyMeta = true
      for (const key of event.keysChanged) if (RECORDING_META_KEYS.has(key)) this.dirtyRecordings = true
    }
    const onAfterTransaction = () => this.flush()
    entries.observeDeep(onEntries)
    lessons.observeDeep(onLessons)
    meta.observe(onMeta)
    doc.on('afterTransaction', onAfterTransaction)
    this.disposers.push(
      () => entries.unobserveDeep(onEntries),
      () => lessons.unobserveDeep(onLessons),
      () => meta.unobserve(onMeta),
      () => doc.off('afterTransaction', onAfterTransaction),
    )
  }

  destroy(): void {
    for (const dispose of this.disposers) dispose()
    this.disposers.length = 0
  }

  private buildStructure(previous: ScriptStructure | null): ScriptStructure {
    const base = readStructure(this.doc)
    const previousLessons = new Map(previous?.lessons.map((lesson) => [lesson.id, lesson]))
    // Keep the previous lesson objects where nothing changed, so memoized
    // lesson components skip re-rendering.
    const lessons: StructureLesson[] = base.lessons.map((lesson) => {
      const old = previousLessons.get(lesson.id)
      return old && sameIds(old.entryIds, lesson.entryIds) ? old : lesson
    })
    const lessonIndex = new Map<string, number>()
    const position = new Map<string, EntryPosition>()
    let flat = 0
    lessons.forEach((lesson, l) => {
      lessonIndex.set(lesson.id, l)
      lesson.entryIds.forEach((id, index) => position.set(id, { lesson: l, index, flat: flat++ }))
    })
    const flatIds = previous && sameIds(previous.flat, base.flat) ? previous.flat : base.flat
    return { lessons, flat: flatIds, orphans: base.orphans, lessonIndex, position }
  }

  private flush(): void {
    const { meta, lessons, entries } = roots(this.doc)
    const notify: Listener[] = []

    for (const id of this.dirtyEntries) {
      const entry = entries.get(id)
      if (entry) this.entries.set(id, entrySnapshot(id, entry))
      else this.entries.delete(id)
      const listeners = this.entryListeners.get(id)
      if (listeners) notify.push(...listeners)
    }
    for (const id of this.dirtyLessons) {
      const lesson = lessons.get(id)
      if (lesson) this.lessons.set(id, lessonSnapshot(id, lesson))
      else this.lessons.delete(id)
      const listeners = this.lessonListeners.get(id)
      if (listeners) notify.push(...listeners)
    }
    if (this.dirtyMeta) {
      this.meta = readMeta(meta)
      notify.push(...this.metaListeners)
    }
    if (this.dirtyStructure) {
      this.structure = this.buildStructure(this.structure)
      notify.push(...this.structureListeners)
    }
    if (this.dirtyRecordings) {
      this.recordingsVersion++
      notify.push(...this.recordingListeners)
    }
    const orphans = this.dirtyStructure ? this.structure.orphans : []

    this.dirtyEntries.clear()
    this.dirtyLessons.clear()
    this.dirtyMeta = false
    this.dirtyStructure = false
    this.dirtyRecordings = false

    for (const listener of notify) listener()
    if (orphans.length > 0 && this.onOrphans) {
      const handler = this.onOrphans
      queueMicrotask(() => handler(orphans))
    }
  }

  // ---- reading -------------------------------------------------------------

  getStructure = (): ScriptStructure => this.structure
  getMeta = (): Meta => this.meta
  getEntry(id: string): EntrySnapshot | undefined {
    return this.entries.get(id)
  }
  getLesson(id: string): LessonSnapshot | undefined {
    return this.lessons.get(id)
  }
  getRecordingsVersion = (): number => this.recordingsVersion

  /** The lesson's recording summary, cached until recordings change. */
  lessonSummary(lessonId: string): string {
    const cached = this.summaries.get(lessonId)
    if (cached && cached.version === this.recordingsVersion) return cached.value
    const lesson = this.structure.lessons.find((item) => item.id === lessonId)
    const infos = (lesson?.entryIds ?? []).map((id) => this.entries.get(id)!).filter(Boolean)
    const value = lessonRecordingSummary(infos, this.meta)
    this.summaries.set(lessonId, { version: this.recordingsVersion, value })
    return value
  }

  // ---- subscribing ---------------------------------------------------------

  subscribeStructure = (listener: Listener): (() => void) => {
    this.structureListeners.add(listener)
    return () => this.structureListeners.delete(listener)
  }

  subscribeMeta = (listener: Listener): (() => void) => {
    this.metaListeners.add(listener)
    return () => this.metaListeners.delete(listener)
  }

  /** Fires when anything a recording summary depends on changes: clips,
   * crops, participants, structure, gaps. */
  subscribeRecordings = (listener: Listener): (() => void) => {
    this.recordingListeners.add(listener)
    return () => this.recordingListeners.delete(listener)
  }

  subscribeEntry(id: string, listener: Listener): () => void {
    return subscribeKeyed(this.entryListeners, id, listener)
  }

  subscribeLesson(id: string, listener: Listener): () => void {
    return subscribeKeyed(this.lessonListeners, id, listener)
  }
}

function subscribeKeyed(map: Map<string, Set<Listener>>, id: string, listener: Listener): () => void {
  let set = map.get(id)
  if (!set) map.set(id, (set = new Set()))
  set.add(listener)
  return () => {
    set.delete(listener)
    if (set.size === 0) map.delete(id)
  }
}
