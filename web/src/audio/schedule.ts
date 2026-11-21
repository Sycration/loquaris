// Lesson playback planning (FEATURES §9): which entries play, where it
// stops, and when each clip starts.
import type { AudioRef, Participant, Trim } from '../doc/schema'
import { effectiveRange } from './crop'

export interface PlanEntry {
  id: string
  participant: Participant
  audio: AudioRef | null
  trim: Trim | null
}

export interface Gaps {
  gapSameMs: number
  gapDifferentMs: number
}

export function gapSeconds(previous: Participant, next: Participant, gaps: Gaps): number {
  return (previous === next ? gaps.gapSameMs : gaps.gapDifferentMs) / 1000
}

export interface PlanItem {
  entryId: string
  audio: AudioRef
  /** Seconds of the cropped clip. */
  duration: number
  /** Seconds from the start of playback to this clip's start. */
  offset: number
}

export interface Plan {
  items: PlanItem[]
  /** Toast text when playback stops at an entry without a clip. */
  stopMessage: string | null
}

/** Plan playback from `startIndex`. Entry numbers in messages are 1-based
 * positions within the lesson. */
export function planPlayback(entries: readonly PlanEntry[], startIndex: number, gaps: Gaps): Plan {
  const items: PlanItem[] = []
  let time = 0
  let stopMessage: string | null = null
  for (let i = startIndex; i < entries.length; i++) {
    const entry = entries[i]
    if (!entry.audio) {
      stopMessage = i === startIndex ? `Entry ${i + 1} has no recording.` : `Playback stopped: entry ${i + 1} has no recording.`
      break
    }
    if (i > startIndex) time += gapSeconds(entries[i - 1].participant, entry.participant, gaps)
    const range = effectiveRange(entry.audio, entry.trim)
    const duration = (range.end - range.start) / Math.max(1, entry.audio.sampleRate)
    items.push({ entryId: entry.id, audio: entry.audio, duration, offset: time })
    time += duration
  }
  return { items, stopMessage }
}
