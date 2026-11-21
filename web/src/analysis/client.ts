// Promise-based access to the analysis worker. Independent of Yjs: the owner
// forwards the document's content and structure, and asks questions.
//
// Every content change (setProject, setStructure, setEntry, removeEntry,
// setL1Name) increments `revision`. Queries resolve to an Answer stamped with
// the revision they were computed for; callers drop answers for which
// `isCurrent` is false. The worker handles requests in order, so an answer
// reflects every change sent before its query.
import type {
  AppliedReview,
  EntryContent,
  FirstUses,
  Gaps,
  Lemma,
  LessonOutline,
  ProjectContent,
  ReviewFlag,
  Request,
  RequestMessage,
  ResponseMessage,
  Results,
  Review,
  ReviewChoices,
  SearchOptions,
  SearchResult,
} from './protocol.ts'

export type * from './protocol.ts'

export interface Answer<T> {
  revision: number
  value: T
}

export interface AnalysisClientOptions {
  /** Creates the worker; tests can substitute their own. */
  createWorker?: () => Worker
  /** Called after the worker crashed and was restarted empty: send the
   * project again with setProject. */
  onRestart?: () => void
}

interface Pending {
  resolve: (answer: Answer<unknown>) => void
  reject: (error: Error) => void
}

const defaultWorker = () =>
  new Worker(new URL('./worker.ts', import.meta.url), { type: 'module', name: 'analysis' })

export class AnalysisClient {
  /** Resolves when the wasm is loaded; rejects if it fails to load. */
  readonly ready: Promise<void>
  private resolveReady!: () => void
  private rejectReady!: (error: Error) => void
  private worker: Worker | null = null
  private nextId = 1
  private currentRevision = 0
  private readonly pending = new Map<number, Pending>()
  private readonly createWorker: () => Worker
  private readonly onRestart?: () => void

  constructor(options: AnalysisClientOptions = {}) {
    this.createWorker = options.createWorker ?? defaultWorker
    this.onRestart = options.onRestart
    this.ready = new Promise<void>((resolve, reject) => {
      this.resolveReady = resolve
      this.rejectReady = reject
    })
    // Callers that never await `ready` shouldn't see an unhandled rejection.
    this.ready.catch(() => {})
  }

  /** The revision of the content last sent. */
  get revision(): number {
    return this.currentRevision
  }

  /** Whether nothing changed since the answer was computed. */
  isCurrent(answer: Answer<unknown>): boolean {
    return answer.revision === this.currentRevision
  }

  /** Start the worker (once) and wait until it is ready. Requests made
   * before then are queued. */
  start(): Promise<void> {
    if (!this.worker) this.spawn()
    return this.ready
  }

  /** Stop the worker; pending requests are rejected. */
  dispose(): void {
    this.worker?.terminate()
    this.worker = null
    this.failPending(new Error('The analysis worker was stopped.'))
  }

  setProject(project: ProjectContent): Promise<void> {
    return this.change({ type: 'setProject', project })
  }

  /** After lessons or entries were moved, inserted or deleted. */
  setStructure(lessons: LessonOutline[]): Promise<void> {
    return this.change({ type: 'setStructure', lessons })
  }

  /** After an entry's text, language, participant or lemma choices changed. */
  setEntry(entry: EntryContent): Promise<void> {
    return this.change({ type: 'setEntry', entry })
  }

  removeEntry(id: string): Promise<void> {
    return this.change({ type: 'removeEntry', id })
  }

  setL1Name(l1Name: string): Promise<void> {
    return this.change({ type: 'setL1Name', l1Name })
  }

  setGaps(gaps: Gaps): Promise<void> {
    return this.change({ type: 'setGaps', gaps })
  }

  /** The spaced repetition flags, in script order. */
  spacedRepetition(): Promise<Answer<ReviewFlag[]>> {
    return this.query({ type: 'spacedRepetition' })
  }

  search(query: string, options: SearchOptions = {}): Promise<Answer<SearchResult>> {
    return this.query({ type: 'search', query, options })
  }

  /** Lemmas first used in each entry (the yellow labels on cards). */
  firstUse(): Promise<Answer<FirstUses>> {
    return this.query({ type: 'firstUse' })
  }

  /** Lemmas first used in the project in this lesson, in script order. */
  newInLesson(lessonId: string): Promise<Answer<Lemma[]>> {
    return this.query({ type: 'newInLesson', lessonId })
  }

  /** The parsing review of a Latin entry; null for other entries. */
  review(entryId: string): Promise<Answer<Review | null>> {
    return this.query({ type: 'review', entryId })
  }

  /** The text and lemma choices to write after a review; null if the
   * entry's text is no longer `original`. */
  applyReview(
    entryId: string,
    original: string,
    choices: ReviewChoices,
  ): Promise<Answer<AppliedReview | null>> {
    return this.query({ type: 'applyReview', entryId, original, choices })
  }

  macronize(text: string): Promise<Answer<string>> {
    return this.query({ type: 'macronize', text })
  }

  private change(request: Request): Promise<void> {
    this.currentRevision += 1
    return this.send(request).then(() => undefined)
  }

  private query<K extends Request['type']>(
    request: Extract<Request, { type: K }>,
  ): Promise<Answer<Results[K]>> {
    return this.send(request) as Promise<Answer<Results[K]>>
  }

  private send(request: Request): Promise<Answer<unknown>> {
    if (!this.worker) this.spawn()
    const id = this.nextId++
    const message: RequestMessage = { id, revision: this.currentRevision, request }
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject })
      this.worker!.postMessage(message)
    })
  }

  private spawn(): void {
    const worker = this.createWorker()
    this.worker = worker
    worker.onmessage = (event: MessageEvent<ResponseMessage>) => this.receive(event.data)
    worker.onerror = (event) => {
      event.preventDefault()
      this.rejectReady(new Error(event.message || 'The analysis worker failed.'))
      this.failPending(new Error(event.message || 'The analysis worker failed.'))
    }
  }

  private receive(message: ResponseMessage): void {
    switch (message.type) {
      case 'ready':
        this.resolveReady()
        return
      case 'failed':
        this.rejectReady(new Error(message.error))
        return
      case 'result': {
        const pending = this.take(message.id)
        if (!pending) return
        const value = message.json === undefined ? undefined : JSON.parse(message.json)
        pending.resolve({ revision: message.revision, value })
        return
      }
      case 'error': {
        this.take(message.id)?.reject(new Error(message.error))
        if (message.fatal) this.restart()
        return
      }
    }
  }

  private take(id: number): Pending | undefined {
    const pending = this.pending.get(id)
    this.pending.delete(id)
    return pending
  }

  /** Replace a worker whose module trapped; its index is lost. */
  private restart(): void {
    this.worker?.terminate()
    this.failPending(new Error('The analysis worker crashed and was restarted.'))
    this.spawn()
    this.onRestart?.()
  }

  private failPending(error: Error): void {
    for (const pending of this.pending.values()) pending.reject(error)
    this.pending.clear()
  }
}
