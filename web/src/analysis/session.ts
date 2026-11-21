// A per-editor analysis session: owns the AnalysisClient, feeds it from the
// DocStore (initial setProject, then incremental updates), and exposes the
// loading state. Independent of React so it can be tested with a fake client.
import { AnalysisClient, type AnalysisClientOptions, type EntryContent, type LemmaChoice, type Gaps, type LessonOutline, type ProjectContent, type ReviewFlag } from './client.ts'
import type { DocStore, EntrySnapshot } from '../doc/projection'
import { clipDuration } from '../doc/summary'
import { SearchController } from './search'
import { Resource } from './resource'

export type AnalysisApi = Pick<
  AnalysisClient,
  | 'start'
  | 'dispose'
  | 'setProject'
  | 'setStructure'
  | 'setEntry'
  | 'removeEntry'
  | 'setL1Name'
  | 'setGaps'
  | 'spacedRepetition'
  | 'search'
  | 'firstUse'
  | 'newInLesson'
  | 'review'
  | 'applyReview'
  | 'isCurrent'
  | 'revision'
>

export type AnalysisStatus = 'idle' | 'loading' | 'ready' | 'failed'

export interface SessionOptions {
  createClient?: (options: AnalysisClientOptions) => AnalysisApi
  /** Delay before text edits reach the worker. */
  debounceMs?: number
}

type Listener = () => void

/** Flags are recomputed this long after the last change. */
const SPACED_DEBOUNCE_MS = 250

/** The worker's view of an entry. Lemma choices are keyed `${token}#${n}`. */
export function entryContent(entry: EntrySnapshot): EntryContent {
  const lemmas: LemmaChoice[] = []
  for (const [key, choice] of Object.entries(entry.lemmas)) {
    const at = key.lastIndexOf('#')
    if (at < 0) continue
    const occurrence = Number(key.slice(at + 1))
    if (!Number.isInteger(occurrence)) continue
    lemmas.push({ token: key.slice(0, at), occurrence, lemmaKey: choice.key, extension: choice.extension })
  }
  lemmas.sort((a, b) => (a.token < b.token ? -1 : a.token > b.token ? 1 : a.occurrence - b.occurrence))
  const content: EntryContent = { id: entry.id, participant: entry.participant, language: entry.language, text: entry.text, lemmas }
  // The cropped clip's length, for spaced repetition timing; none without a recording.
  if (entry.audio) content.duration = clipDuration(entry.audio, entry.trim)
  return content
}

function signature(content: EntryContent): string {
  return JSON.stringify(content)
}

export class AnalysisSession {
  readonly store: DocStore
  private readonly options: SessionOptions
  private client: AnalysisApi | null = null
  private startPromise: Promise<void> | null = null
  private generation = 0
  private _status: AnalysisStatus = 'idle'
  private _error: string | null = null
  private _version = 0
  private listeners = new Set<Listener>()

  private sent = new Map<string, string>() // entry id → signature last sent
  private entryUnsubs = new Map<string, () => void>()
  private dirty = new Set<string>()
  private timer: ReturnType<typeof setTimeout> | null = null
  private unsubs: (() => void)[] = []
  private l1Name = ''
  private gaps: Gaps = { sameParticipantMs: 0, differentParticipantMs: 0 }
  private spacedOn = true

  readonly search: SearchController
  readonly firstUses: Resource<Record<string, string[]>>
  /** Spaced repetition flags; empty while the project switches them off. */
  readonly spaced: Resource<ReviewFlag[]>

  constructor(store: DocStore, options: SessionOptions = {}) {
    this.store = store
    this.options = options
    this.search = new SearchController(this)
    this.firstUses = new Resource(this, (client) => client.firstUse())
    this.spaced = new Resource(
      this,
      (client) =>
        this.store.getMeta().spacedRepetition === false
          ? Promise.resolve({ revision: client.revision, value: [] })
          : client.spacedRepetition(),
      SPACED_DEBOUNCE_MS,
    )
  }

  get status(): AnalysisStatus {
    return this._status
  }
  get error(): string | null {
    return this._error
  }
  /** Increments whenever content was sent to the worker or its status
   * changed: queries are re-run when it does. */
  get version(): number {
    return this._version
  }
  get api(): AnalysisApi | null {
    return this.client
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  private bump(): void {
    this._version++
    for (const listener of [...this.listeners]) listener()
  }

  /** Start the worker (once) and resolve when it is ready. */
  start(): Promise<void> {
    if (this.startPromise) return this.startPromise
    const generation = ++this.generation
    const client = (this.options.createClient ?? ((o) => new AnalysisClient(o)))({ onRestart: () => this.resend() })
    this.client = client
    this._status = 'loading'
    this._error = null
    this.attach()
    this.resend()
    this.bump()
    this.startPromise = client.start().then(
      () => {
        if (generation !== this.generation) return
        this._status = 'ready'
        this.bump()
      },
      (error: unknown) => {
        if (generation !== this.generation) return
        this._status = 'failed'
        this._error = error instanceof Error ? error.message : String(error)
        this.bump()
      },
    )
    return this.startPromise
  }

  /** Stop the worker and all subscriptions; start() can run again. */
  dispose(): void {
    this.generation++
    this.detach()
    this.client?.dispose()
    this.client = null
    this.startPromise = null
    this._status = 'idle'
    this.sent.clear()
    this.search.reset()
    this.firstUses.reset()
    this.spaced.reset()
    this.bump()
  }

  /** Send pending text edits now (before a query that must see them). */
  flush(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    const client = this.client
    if (!client) return
    let sentAny = false
    for (const id of this.dirty) {
      const entry = this.store.getEntry(id)
      if (!entry || !this.entryUnsubs.has(id)) continue
      const content = entryContent(entry)
      const sig = signature(content)
      if (this.sent.get(id) === sig) continue
      this.sent.set(id, sig)
      this.run(client.setEntry(content))
      sentAny = true
    }
    this.dirty.clear()
    if (sentAny) this.bump()
  }

  private run(promise: Promise<unknown>): void {
    promise.catch(() => {
      // Failures are rejected requests of a worker that was restarted or
      // stopped; the restart path resends everything.
    })
  }

  private snapshotProject(): ProjectContent {
    const structure = this.store.getStructure()
    const meta = this.store.getMeta()
    this.l1Name = meta.l1Name
    this.spacedOn = meta.spacedRepetition !== false
    return {
      l1Name: this.l1Name,
      lessons: structure.lessons.map((lesson) => ({
        id: lesson.id,
        entries: lesson.entryIds.flatMap((id) => {
          const entry = this.store.getEntry(id)
          if (!entry) return []
          const content = entryContent(entry)
          this.sent.set(id, signature(content))
          return [content]
        }),
      })),
    }
  }

  private snapshotGaps(): Gaps {
    const meta = this.store.getMeta()
    this.gaps = { sameParticipantMs: meta.gapSameMs, differentParticipantMs: meta.gapDifferentMs }
    return this.gaps
  }

  /** Initial project, and again after the worker restarted empty. */
  private resend(): void {
    const client = this.client
    if (!client) return
    this.sent.clear()
    this.dirty.clear()
    const project = this.snapshotProject()
    for (const id of this.store.getStructure().flat) this.watchEntry(id)
    this.run(client.setProject(project))
    this.run(client.setGaps(this.snapshotGaps()))
  }

  private attach(): void {
    this.detach()
    this.unsubs.push(
      this.store.subscribeStructure(() => this.onStructure()),
      this.store.subscribeMeta(() => this.onMeta()),
    )
  }

  private detach(): void {
    for (const unsub of this.unsubs) unsub()
    this.unsubs = []
    for (const unsub of this.entryUnsubs.values()) unsub()
    this.entryUnsubs.clear()
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.dirty.clear()
  }

  private watchEntry(id: string): void {
    if (this.entryUnsubs.has(id)) return
    this.entryUnsubs.set(
      id,
      this.store.subscribeEntry(id, () => {
        this.dirty.add(id)
        if (this.timer) clearTimeout(this.timer)
        this.timer = setTimeout(() => this.flush(), this.options.debounceMs ?? 100)
      }),
    )
  }

  private onStructure(): void {
    const client = this.client
    if (!client) return
    const structure = this.store.getStructure()
    const present = new Set(structure.flat)
    for (const id of [...this.entryUnsubs.keys()]) {
      if (present.has(id)) continue
      this.entryUnsubs.get(id)!()
      this.entryUnsubs.delete(id)
      this.sent.delete(id)
      this.dirty.delete(id)
      this.run(client.removeEntry(id))
    }
    for (const id of structure.flat) {
      if (this.entryUnsubs.has(id)) continue
      this.watchEntry(id)
      const entry = this.store.getEntry(id)
      if (!entry) continue
      const content = entryContent(entry)
      this.sent.set(id, signature(content))
      this.run(client.setEntry(content))
    }
    const lessons: LessonOutline[] = structure.lessons.map((lesson) => ({ id: lesson.id, entryIds: [...lesson.entryIds] }))
    this.run(client.setStructure(lessons))
    this.bump()
  }

  private onMeta(): void {
    const client = this.client
    if (!client) return
    const meta = this.store.getMeta()
    let changed = false
    if (meta.l1Name !== this.l1Name) {
      this.l1Name = meta.l1Name
      this.run(client.setL1Name(meta.l1Name))
      changed = true
    }
    if (meta.gapSameMs !== this.gaps.sameParticipantMs || meta.gapDifferentMs !== this.gaps.differentParticipantMs) {
      this.run(client.setGaps(this.snapshotGaps()))
      changed = true
    }
    const spacedOn = meta.spacedRepetition !== false
    if (spacedOn !== this.spacedOn) {
      this.spacedOn = spacedOn
      changed = true
    }
    if (changed) this.bump()
  }
}

const sessions = new WeakMap<DocStore, AnalysisSession>()

/** The editor's session (one per DocStore), created on first use. */
export function sessionFor(store: DocStore): AnalysisSession {
  let session = sessions.get(store)
  if (!session) sessions.set(store, (session = new AnalysisSession(store)))
  return session
}
