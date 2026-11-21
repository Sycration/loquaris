// The recorder's engine for one entry: capture, upload, clip loading, crop
// and preview playback. React reads it through subscribe/getState.
import { api } from '../api/client'
import type { AudioRef } from '../doc/schema'
import type { EntrySnapshot } from '../doc/projection'
import type { EditorServices } from '../editor/context'
import { confirm } from '../ui/Dialog'
import { errorMessage, toast } from '../ui/toasts'
import { Capture } from './capture'
import { loadClip, primeClip, type LoadedClip } from './clips'
import { playbackContext } from './context'
import { clampCrop, effectiveRange, normalizeTrim, playbackStart, rampsFor, sameTrim, seek, SEEK_SECONDS } from './crop'
import { getSavedDevice } from './devices'
import { setEntryAudio, setEntryTrim } from './docEdit'
import { encodeWav, MAX_IMPORT_BYTES, parseWavHeader } from './wav'
import { peakAmplitude, processTake } from './dsp'
import { playSegment, type Segment } from './segment'
import { stopLessonPlayback } from './lessonPlayer'

export interface Diagnostics {
  callbacks: number
  samples: number
  /** 0..100 */
  peakPercent: number
  elapsed: number
  sampleRate: number
  channels: number
}

export interface SessionState {
  phase: 'idle' | 'starting' | 'recording' | 'uploading'
  playing: boolean
  /** Playhead, in frames of the clip. */
  cursor: number
  /** Seconds recorded so far. */
  elapsed: number
  diagnostics: Diagnostics | null
  clip: LoadedClip | null
  clipError: string | null
  /** Bumped on every live update so the waveform redraws. */
  tick: number
  error: string | null
}

const INITIAL: SessionState = {
  phase: 'idle',
  playing: false,
  cursor: 0,
  elapsed: 0,
  diagnostics: null,
  clip: null,
  clipError: null,
  tick: 0,
  error: null,
}

let active: RecorderSession | null = null

/** True while this user is recording (undo / redo are refused). */
export function isRecordingNow(): boolean {
  return active?.getState().phase === 'recording'
}

const recordingListeners = new Set<() => void>()
export const recordingChanges = {
  subscribe(listener: () => void) {
    recordingListeners.add(listener)
    return () => {
      recordingListeners.delete(listener)
    }
  },
}

export function diagnosticsText(d: Diagnostics): string {
  return [
    `Callbacks: ${d.callbacks}`,
    `Samples: ${d.samples}`,
    `Peak: ${d.peakPercent.toFixed(1)}%`,
    `Elapsed: ${d.elapsed.toFixed(1)} s`,
    `Sample rate: ${d.sampleRate} Hz`,
    `Channels: ${d.channels}`,
  ].join('\n')
}

export class RecorderSession {
  private state = INITIAL
  private listeners = new Set<() => void>()
  private capture: Capture | null = null
  private segment: Segment | null = null
  private playToken = 0
  private playStartTime = 0
  private playStartFrame = 0
  private raf = 0
  private startBlob: string | null = null
  private disposers: (() => void)[] = []
  private blob: string | null | undefined = undefined
  private trimKey: string | null = null
  private disposed = false
  private discarding = false

  private readonly services: EditorServices
  readonly entryId: string

  constructor(services: EditorServices, entryId: string) {
    this.services = services
    this.entryId = entryId
    active = this
    stopLessonPlayback()
    const unsubscribe = services.store.subscribeEntry(entryId, () => this.onEntryChange())
    this.disposers.push(unsubscribe)
    this.onEntryChange()
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  getState = () => this.state

  get entry(): EntrySnapshot | undefined {
    return this.services.store.getEntry(this.entryId)
  }

  private set(patch: Partial<SessionState>) {
    const wasRecording = this.state.phase === 'recording'
    this.state = { ...this.state, ...patch }
    for (const listener of this.listeners) listener()
    if (wasRecording !== (this.state.phase === 'recording')) for (const listener of recordingListeners) listener()
  }

  private live(patch: Partial<SessionState> = {}) {
    this.set({ ...patch, tick: this.state.tick + 1 })
  }

  // ---- the entry ---------------------------------------------------------

  private onEntryChange() {
    const entry = this.entry
    if (!entry) {
      if (this.state.phase === 'recording' || this.state.phase === 'starting') void this.discard('The entry was deleted, so the take was discarded.')
      return
    }
    const blob = entry.audio?.blob ?? null
    const trimKey = entry.trim ? `${entry.trim.start}-${entry.trim.end}` : null
    if (blob !== this.blob) {
      this.blob = blob
      this.trimKey = trimKey
      this.stopPlayback()
      this.loadCurrentClip(entry.audio)
    } else if (trimKey !== this.trimKey) {
      this.trimKey = trimKey
      this.stopPlayback()
      this.set({ cursor: this.clipRange().start })
    }
  }

  private loadCurrentClip(audio: AudioRef | null) {
    if (!audio) {
      this.set({ clip: null, clipError: null, cursor: 0 })
      return
    }
    this.set({ clip: null, clipError: null, cursor: 0 })
    loadClip(audio).then(
      (clip) => {
        if (this.disposed || this.blob !== audio.blob) return
        this.set({ clip, cursor: effectiveRange(clip, this.entry?.trim ?? null).start })
      },
      (error) => {
        if (!this.disposed && this.blob === audio.blob) this.set({ clipError: errorMessage(error) })
      },
    )
  }

  clipRange() {
    const clip = this.state.clip
    return clip ? effectiveRange(clip, this.entry?.trim ?? null) : { start: 0, end: 0 }
  }

  // ---- crop --------------------------------------------------------------

  /** Apply a dragged crop on handle release. */
  commitCrop(range: { start: number; end: number }, moving: 'start' | 'end') {
    const clip = this.state.clip
    if (!clip || this.services.readOnly) return
    const next = normalizeTrim(clip, clampCrop(clip, range, moving))
    const current = this.entry?.trim ?? null
    if (sameTrim(next, current)) return
    setEntryTrim(this.services, this.entryId, next)
  }

  // ---- preview playback --------------------------------------------------

  play(from?: number) {
    const clip = this.state.clip
    if (!clip || this.state.phase !== 'idle') return
    this.stopSource()
    const range = this.clipRange()
    const start = playbackStart(from ?? this.state.cursor, range)
    const context = playbackContext()
    const token = ++this.playToken
    const trimmed = { frames: clip.frames, sampleRate: clip.sampleRate }
    this.playStartTime = context.currentTime + 0.02
    this.playStartFrame = start
    this.segment = playSegment(context, clip.buffer, start, range.end, rampsFor(trimmed, range, start), this.playStartTime, () => {
      if (token !== this.playToken) return
      this.stopPlayback()
    })
    this.set({ playing: true, cursor: start })
    this.loop(token)
  }

  private loop(token: number) {
    cancelAnimationFrame(this.raf)
    const step = () => {
      if (token !== this.playToken || !this.state.playing) return
      this.set({ cursor: this.currentFrame() })
      this.raf = requestAnimationFrame(step)
    }
    this.raf = requestAnimationFrame(step)
  }

  private currentFrame(): number {
    const clip = this.state.clip
    if (!clip) return 0
    const elapsed = Math.max(0, playbackContext().currentTime - this.playStartTime)
    return Math.min(this.clipRange().end, Math.round(this.playStartFrame + elapsed * clip.sampleRate))
  }

  private stopSource() {
    this.playToken++
    cancelAnimationFrame(this.raf)
    this.segment?.stop()
    this.segment = null
  }

  /** Stop and return to the crop start. */
  stopPlayback() {
    const wasPlaying = this.state.playing
    this.stopSource()
    if (wasPlaying || this.state.cursor !== this.clipRange().start) this.set({ playing: false, cursor: this.clipRange().start })
  }

  togglePlay() {
    if (this.state.phase !== 'idle' || !this.state.clip) return
    if (this.state.playing) {
      const cursor = this.currentFrame()
      this.stopSource()
      this.set({ playing: false, cursor })
    } else this.play()
  }

  toStart() {
    if (!this.state.clip || this.state.phase !== 'idle') return
    this.moveCursor(this.clipRange().start)
  }

  seekBy(seconds: number) {
    const clip = this.state.clip
    if (!clip || this.state.phase !== 'idle') return
    const cursor = this.state.playing ? this.currentFrame() : this.state.cursor
    this.moveCursor(seek(cursor, seconds, clip, this.clipRange()))
  }

  seekToEnd() {
    if (!this.state.clip || this.state.phase !== 'idle') return
    this.moveCursor(this.clipRange().end)
  }

  moveCursor(frame: number) {
    const clip = this.state.clip
    if (!clip) return
    const target = Math.min(clip.frames, Math.max(0, Math.round(frame)))
    if (this.state.playing) {
      // Seeking to the very end of the crop just ends playback there.
      if (target >= this.clipRange().end) {
        this.stopSource()
        this.set({ playing: false, cursor: target })
      } else this.play(target)
    } else this.set({ cursor: target })
  }

  seekBack() {
    this.seekBy(-SEEK_SECONDS)
  }
  seekForward() {
    this.seekBy(SEEK_SECONDS)
  }

  // ---- recording ---------------------------------------------------------

  async record() {
    if (this.state.phase !== 'idle' || this.services.readOnly || !this.entry) return
    this.stopPlayback()
    stopLessonPlayback()
    this.startBlob = this.entry.audio?.blob ?? null
    this.set({ phase: 'starting', error: null, diagnostics: null })
    try {
      const capture = await Capture.start(
        getSavedDevice(),
        () => void this.discard('Recording exceeded the 10 minute limit.', 'error'),
        () => this.onCaptureActivity(),
      )
      if (this.disposed || this.getState().phase !== 'starting') {
        void capture.stop()
        return
      }
      this.capture = capture
      this.services.presence.setRecording(this.entryId)
      this.set({ phase: 'recording', elapsed: 0, diagnostics: this.diagnosticsOf(capture) })
      this.startTimer()
    } catch (error) {
      const denied = error instanceof DOMException && (error.name === 'NotAllowedError' || error.name === 'SecurityError')
      const message = denied
        ? 'Microphone access was blocked. Allow it in the browser’s site settings.'
        : error instanceof DOMException && error.name === 'NotFoundError'
          ? 'No microphone was found.'
          : `Could not start recording: ${errorMessage(error)}`
      this.set({ phase: 'idle', error: message })
    }
  }

  private timer = 0
  private startTimer() {
    window.clearInterval(this.timer)
    // Silent streams send few callbacks; keep the elapsed time and warnings fresh.
    this.timer = window.setInterval(() => this.onCaptureActivity(), 250)
  }

  private diagnosticsOf(capture: Capture): Diagnostics {
    const { stats } = capture
    return {
      callbacks: stats.callbacks,
      samples: stats.samples,
      peakPercent: Math.min(100, stats.peak * 100),
      elapsed: capture.elapsed,
      sampleRate: stats.sampleRate,
      channels: stats.channels,
    }
  }

  private onCaptureActivity() {
    if (!this.capture) return
    this.live({ elapsed: this.capture.elapsed, diagnostics: this.diagnosticsOf(this.capture) })
  }

  get livePeaks() {
    return this.capture?.peaks ?? null
  }

  /** Stop recording; the take is processed, uploaded and applied. */
  async stopRecording() {
    const capture = this.capture
    if (!capture || this.state.phase !== 'recording') return
    this.capture = null
    window.clearInterval(this.timer)
    this.services.presence.setRecording(null)
    const stats: Diagnostics = this.diagnosticsOf(capture)
    this.set({ phase: 'uploading', diagnostics: stats })
    try {
      const { channels, sampleRate } = await capture.stop()
      if (channels.length === 0 || channels[0].length === 0) {
        toast.info('No audio was recorded.')
        this.set({ phase: 'idle' })
        return
      }
      const processed = processTake(channels, sampleRate)
      const wav = encodeWav(processed, sampleRate)
      this.set({ diagnostics: { ...stats, peakPercent: Math.min(100, peakAmplitude(channels) * 100) } })
      const audio = await api.projects.uploadAudio(this.services.projectId, new Blob([wav], { type: 'audio/wav' }))
      primeClip(audio.blob, processed, sampleRate)
      await this.apply(audio, 'Saved the recording.')
    } catch (error) {
      toast.error(`Could not save the recording: ${errorMessage(error)}`)
      this.set({ phase: 'idle' })
    }
  }

  /** Stop and throw the take away. */
  private async discard(message: string, level: 'info' | 'error' = 'info') {
    if (this.discarding) return
    this.discarding = true
    const capture = this.capture
    this.capture = null
    window.clearInterval(this.timer)
    this.services.presence.setRecording(null)
    toast[level](message)
    this.set({ phase: 'idle' })
    await capture?.stop().catch(() => {})
    this.discarding = false
    this.onClosedWhileIdle?.()
  }

  /** Called when the take was discarded (so the dialog can close). */
  onClosedWhileIdle: (() => void) | null = null

  // ---- applying clips ----------------------------------------------------

  private async apply(audio: AudioRef, successMessage: string | null) {
    const entry = this.entry
    if (!entry) {
      toast.info('The entry was deleted, so the take was discarded.')
      this.set({ phase: 'idle' })
      return
    }
    const currentBlob = entry.audio?.blob ?? null
    if (currentBlob !== this.startBlob) {
      const useMine = await confirm({
        title: 'Someone else changed this recording',
        message: 'Another collaborator replaced this entry’s clip while you were working. Replace it with your take?',
        confirmLabel: 'Use my take',
        cancelLabel: 'Keep theirs',
      })
      if (!useMine) {
        toast.info('Kept the other clip.')
        this.set({ phase: 'idle' })
        return
      }
    }
    if (!setEntryAudio(this.services, this.entryId, audio)) {
      toast.info('The entry was deleted, so the take was discarded.')
    } else if (successMessage) toast.success(successMessage)
    this.set({ phase: 'idle' })
  }

  async importWav(file: File) {
    if (this.state.phase !== 'idle' || this.services.readOnly) return
    if (file.size > MAX_IMPORT_BYTES) {
      toast.error('WAV files can be at most 100 MiB.')
      return
    }
    this.startBlob = this.entry?.audio?.blob ?? null
    this.set({ phase: 'uploading' })
    try {
      const bytes = new Uint8Array(await file.arrayBuffer())
      if (!parseWavHeader(bytes)) {
        toast.error('That file isn’t a WAV file.')
        this.set({ phase: 'idle' })
        return
      }
      this.stopPlayback()
      const audio = await api.projects.uploadAudio(this.services.projectId, new Blob([bytes], { type: 'audio/wav' }))
      await this.apply(audio, 'Imported the WAV file.')
    } catch (error) {
      toast.error(`Could not import the WAV file: ${errorMessage(error)}`)
      this.set({ phase: 'idle' })
    }
  }

  deleteClip() {
    if (this.state.phase !== 'idle' || this.services.readOnly || !this.entry?.audio) return
    this.stopPlayback()
    setEntryAudio(this.services, this.entryId, null)
  }

  /** Dialog closed: a take in progress is stopped and kept. */
  close() {
    const phase: SessionState['phase'] = this.state.phase
    if (phase === 'recording') void this.stopRecording()
    else if (phase === 'starting') this.set({ phase: 'idle' })
    this.stopSource()
    this.set({ playing: false })
  }

  /** Release everything once nothing is pending. */
  dispose() {
    this.disposed = true
    this.stopSource()
    window.clearInterval(this.timer)
    for (const dispose of this.disposers) dispose()
    this.disposers = []
    if (this.capture) void this.capture.stop().catch(() => {})
    this.services.presence.setRecording(null)
    if (active === this) active = null
  }
}
