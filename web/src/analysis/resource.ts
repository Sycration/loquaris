// A query whose latest current answer is shared by every subscriber (first-
// use labels on all cards share one). Re-run when the session's content
// version changes; answers computed for an older revision are dropped.
import type { Answer } from './client.ts'
import type { AnalysisApi, AnalysisSession } from './session'

type Listener = () => void

export class Resource<T> {
  private value: T | null = null
  private listeners = new Set<Listener>()
  private unsubSession: (() => void) | null = null
  private sequence = 0

  private readonly session: AnalysisSession
  private readonly fetch: (client: AnalysisApi) => Promise<Answer<T>>

  private readonly debounceMs: number
  private timer: ReturnType<typeof setTimeout> | null = null

  /** With `debounceMs`, a refresh waits that long after the last request. */
  constructor(session: AnalysisSession, fetch: (client: AnalysisApi) => Promise<Answer<T>>, debounceMs = 0) {
    this.session = session
    this.fetch = fetch
    this.debounceMs = debounceMs
  }

  getSnapshot = (): T | null => this.value

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    if (!this.unsubSession) {
      this.unsubSession = this.session.subscribe(() => this.refresh())
      this.refresh()
    }
    return () => {
      this.listeners.delete(listener)
      if (this.listeners.size === 0) {
        this.unsubSession?.()
        this.unsubSession = null
      }
    }
  }

  reset(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    this.sequence++
    this.value = null
  }

  refresh(): void {
    if (this.debounceMs <= 0) return this.fetchNow()
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => {
      this.timer = null
      this.fetchNow()
    }, this.debounceMs)
  }

  private fetchNow(): void {
    const client = this.session.api
    if (!client || this.session.status !== 'ready') return
    const sequence = ++this.sequence
    this.fetch(client).then(
      (answer) => {
        if (sequence !== this.sequence || this.session.api !== client || !client.isCurrent(answer)) return
        this.value = answer.value
        for (const listener of [...this.listeners]) listener()
      },
      () => {},
    )
  }
}
