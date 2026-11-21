// Lesson playback (FEATURES §9) on Web Audio. All clips are decoded first,
// then scheduled on the audio clock with the participant gaps; a timer moves
// the selection as the clock passes each clip. Pausing suspends the context.
import type { EditorServices } from '../editor/context'
import { toast, errorMessage } from '../ui/toasts'
import { playbackContext } from './context'
import { loadClip } from './clips'
import { effectiveRange, rampsFor } from './crop'
import { planPlayback, type Plan } from './schedule'
import { playSegment, type Segment } from './segment'

export type LessonPlayState = { lessonId: string | null; status: 'idle' | 'loading' | 'playing' | 'paused' }

const IDLE: LessonPlayState = { lessonId: null, status: 'idle' }
let state = IDLE
const listeners = new Set<() => void>()
let current: Run | null = null

export const lessonPlayState = {
  subscribe(listener: () => void) {
    listeners.add(listener)
    return () => {
      listeners.delete(listener)
    }
  },
  get: () => state,
}

function setState(next: LessonPlayState) {
  state = next
  for (const listener of listeners) listener()
}

class Run {
  private segments: Segment[] = []
  private timer = 0
  private t0 = 0
  private plan: Plan | null = null
  private index = -1
  private selecting = false
  private disposers: (() => void)[] = []
  private ended = false

  private readonly services: EditorServices
  readonly lessonId: string

  constructor(services: EditorServices, lessonId: string) {
    this.services = services
    this.lessonId = lessonId
  }

  async start(): Promise<void> {
    const { store, commands } = this.services
    const entryIds = store.getStructure().lessons.find((lesson) => lesson.id === this.lessonId)?.entryIds ?? []
    const entries = entryIds.flatMap((id) => {
      const entry = store.getEntry(id)
      return entry ? [entry] : []
    })
    const selected = commands.primary()
    const startIndex = Math.max(0, selected ? entries.findIndex((entry) => entry.id === selected) : 0)
    const meta = store.getMeta()
    const plan = planPlayback(entries, startIndex, meta)
    if (plan.items.length === 0) {
      if (plan.stopMessage) toast.info(plan.stopMessage)
      this.finish(false)
      return
    }
    this.plan = plan
    setState({ lessonId: this.lessonId, status: 'loading' })
    let buffers: AudioBuffer[]
    try {
      buffers = (await Promise.all(plan.items.map((item) => loadClip(item.audio)))).map((clip) => clip.buffer)
    } catch (error) {
      if (!this.ended) toast.error(`Could not play the lesson: ${errorMessage(error)}`)
      this.finish(false)
      return
    }
    if (this.ended) return

    const context = playbackContext()
    if (context.state === 'suspended') await context.resume()
    if (this.ended) return
    this.t0 = context.currentTime + 0.08
    plan.items.forEach((item, i) => {
      const buffer = buffers[i]
      const entry = store.getEntry(item.entryId)!
      const range = effectiveRange(item.audio, entry.trim)
      // Scale frames if the decoded rate differs from the stored one.
      const scale = buffer.sampleRate / Math.max(1, item.audio.sampleRate)
      const start = Math.round(range.start * scale)
      const end = Math.min(buffer.length, Math.round(range.end * scale))
      this.segments.push(
        playSegment(context, buffer, start, end, rampsFor({ frames: item.audio.frames, sampleRate: item.audio.sampleRate }, range), this.t0 + item.offset),
      )
    })

    this.disposers.push(
      this.services.ui.subscribe(() => {
        if (this.selecting || this.ended) return
        if (this.services.commands.primary() !== this.plan?.items[this.index]?.entryId) this.finish(true)
      }),
      store.subscribeRecordings(() => this.finish(true)),
    )
    setState({ lessonId: this.lessonId, status: 'playing' })
    this.tick()
    this.timer = window.setInterval(() => this.tick(), 40)
  }

  private tick() {
    if (this.ended || !this.plan) return
    const context = playbackContext()
    const elapsed = context.currentTime - this.t0
    const { items } = this.plan
    let index = this.index
    while (index + 1 < items.length && elapsed >= items[index + 1].offset) index++
    if (index !== this.index && index >= 0) {
      this.index = index
      this.selecting = true
      try {
        this.services.commands.reveal(items[index].entryId)
      } finally {
        this.selecting = false
      }
    }
    const last = items[items.length - 1]
    if (elapsed >= last.offset + last.duration) {
      if (this.plan.stopMessage) toast.info(this.plan.stopMessage)
      this.finish(false)
    }
  }

  pause() {
    if (state.status !== 'playing') return
    void playbackContext().suspend()
    setState({ lessonId: this.lessonId, status: 'paused' })
  }

  resume() {
    if (state.status !== 'paused') return
    void playbackContext().resume()
    setState({ lessonId: this.lessonId, status: 'playing' })
  }

  finish(_interrupted: boolean) {
    if (this.ended) return
    this.ended = true
    window.clearInterval(this.timer)
    for (const dispose of this.disposers) dispose()
    for (const segment of this.segments) segment.stop()
    const context = playbackContext()
    if (context.state === 'suspended') void context.resume()
    if (current === this) {
      current = null
      setState(IDLE)
    }
  }
}

/** Start (or toggle) playback of a lesson. */
export function togglePlayLesson(services: EditorServices, lessonId: string): void {
  if (current && current.lessonId === lessonId) {
    if (state.status === 'playing') current.pause()
    else if (state.status === 'paused') current.resume()
    return
  }
  stopLessonPlayback()
  const run = new Run(services, lessonId)
  current = run
  setState({ lessonId, status: 'loading' })
  void run.start()
}

export function stopLessonPlayback(): void {
  current?.finish(true)
}
