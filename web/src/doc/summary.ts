// Port of loquaris_core::model::lesson_recording_summary (FEATURES §3).
import type { AudioRef, Participant, Trim } from './schema'

export interface RecordingInfo {
  participant: Participant
  audio: AudioRef | null
  trim: Trim | null
}

export interface Gaps {
  gapSameMs: number
  gapDifferentMs: number
}

/** Seconds of the cropped clip. */
export function clipDuration(audio: AudioRef, trim: Trim | null): number {
  const frames = trim ? trim.end - trim.start : audio.frames
  return frames / Math.max(1, audio.sampleRate)
}

/** "Recordings incomplete" if the lesson is empty or any entry lacks a clip;
 * otherwise the total of cropped durations plus gaps, as `MM:SS.t`, or
 * `H:MM:SS.t` from one hour. */
export function lessonRecordingSummary(entries: RecordingInfo[], gaps: Gaps): string {
  if (entries.length === 0) return 'Recordings incomplete'
  let total = 0
  for (const entry of entries) {
    if (!entry.audio) return 'Recordings incomplete'
    total += clipDuration(entry.audio, entry.trim)
  }
  for (let i = 1; i < entries.length; i++) {
    const same = entries[i - 1].participant === entries[i].participant
    total += (same ? gaps.gapSameMs : gaps.gapDifferentMs) / 1000
  }
  return formatTenths(total)
}

export function formatTenths(totalSeconds: number): string {
  const tenths = Math.round(totalSeconds * 10)
  const seconds = Math.floor(tenths / 10)
  const pad = (n: number) => String(n).padStart(2, '0')
  if (seconds >= 3600) {
    return `${Math.floor(seconds / 3600)}:${pad(Math.floor(seconds / 60) % 60)}:${pad(seconds % 60)}.${tenths % 10}`
  }
  return `${pad(Math.floor(seconds / 60))}:${pad(seconds % 60)}.${tenths % 10}`
}
