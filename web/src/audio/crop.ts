// Crop and playback arithmetic (FEATURES §10, §11): trims are frame ranges
// with an exclusive end, at least 50 ms wide, and a trim covering the whole
// clip is stored as none.
import type { Trim } from '../doc/schema'

export const MIN_CROP_SECONDS = 0.05
export const RAMP_SECONDS = 0.01
export const SEEK_SECONDS = 5
export const MAX_TAKE_SECONDS = 600

export interface ClipInfo {
  frames: number
  sampleRate: number
}

export function minCropFrames(clip: ClipInfo): number {
  return Math.min(clip.frames, Math.max(1, Math.ceil(MIN_CROP_SECONDS * clip.sampleRate)))
}

/** The effective [start, end) of a clip: out-of-range trims are clamped,
 * keeping at least one frame. */
export function effectiveRange(clip: ClipInfo, trim: Trim | null): { start: number; end: number } {
  if (!trim || clip.frames <= 0) return { start: 0, end: Math.max(0, clip.frames) }
  const frames = clip.frames
  const start = Math.min(Math.max(0, Math.floor(trim.start)), frames - 1)
  const end = Math.min(Math.max(Math.floor(trim.end), start + 1), frames)
  return { start, end }
}

/** Clamp a dragged crop to the clip and the minimum width, moving only the
 * handle being dragged. */
export function clampCrop(clip: ClipInfo, range: { start: number; end: number }, moving: 'start' | 'end'): Trim {
  const min = minCropFrames(clip)
  let { start, end } = range
  if (moving === 'start') {
    start = Math.round(Math.min(Math.max(0, start), Math.max(0, end - min)))
    end = Math.min(Math.max(end, start + min), clip.frames)
  } else {
    end = Math.round(Math.max(Math.min(clip.frames, end), Math.min(clip.frames, start + min)))
    start = Math.max(0, Math.min(start, end - min))
  }
  return { start, end }
}

/** The trim to store: null when it covers the whole clip. */
export function normalizeTrim(clip: ClipInfo, trim: Trim): Trim | null {
  const { start, end } = effectiveRange(clip, trim)
  return start <= 0 && end >= clip.frames ? null : { start, end }
}

export function sameTrim(a: Trim | null, b: Trim | null): boolean {
  return a === b || (a !== null && b !== null && a.start === b.start && a.end === b.end)
}

/** Where recorder playback starts: from the cursor, or the crop start when
 * the cursor is outside the crop (FEATURES §10). Frames. */
export function playbackStart(cursor: number, range: { start: number; end: number }): number {
  return cursor >= range.start && cursor < range.end ? cursor : range.start
}

/** Seek by `seconds` (negative: back), clamped to the crop. */
export function seek(cursor: number, seconds: number, clip: ClipInfo, range: { start: number; end: number }): number {
  return Math.min(range.end, Math.max(range.start, Math.round(cursor + seconds * clip.sampleRate)))
}

export interface Ramps {
  /** Ramp the start up (the crop cuts into the clip's beginning). */
  fadeIn: boolean
  /** Ramp the end down. */
  fadeOut: boolean
}

/** 10 ms ramps apply at crop edges that cut into the clip, not at the
 * clip's own ends. `from` is the frame playback starts at (a seek into the
 * crop is not an edge). */
export function rampsFor(clip: ClipInfo, range: { start: number; end: number }, from: number = range.start): Ramps {
  return { fadeIn: range.start > 0 && from <= range.start, fadeOut: range.end < clip.frames }
}

/** Piecewise-linear gain envelope for playing `duration` seconds:
 * [time, gain] breakpoints. */
export function rampEnvelope(duration: number, ramps: Ramps, ramp = RAMP_SECONDS): [number, number][] {
  const r = Math.min(ramp, duration / 2)
  const points: [number, number][] = []
  points.push([0, ramps.fadeIn ? 0 : 1])
  if (ramps.fadeIn) points.push([r, 1])
  if (ramps.fadeOut) points.push([duration - r, 1], [duration, 0])
  else points.push([duration, 1])
  return points
}

/** Gain at `time` seconds into a clip of `duration`. */
export function gainAt(time: number, duration: number, ramps: Ramps, ramp = RAMP_SECONDS): number {
  const points = rampEnvelope(duration, ramps, ramp)
  if (time <= points[0][0]) return points[0][1]
  for (let i = 1; i < points.length; i++) {
    const [t1, g1] = points[i]
    if (time <= t1) {
      const [t0, g0] = points[i - 1]
      return t1 === t0 ? g1 : g0 + ((g1 - g0) * (time - t0)) / (t1 - t0)
    }
  }
  return points[points.length - 1][1]
}

/** MM:SS.hh */
export function formatClock(seconds: number): string {
  const centis = Math.max(0, Math.round(seconds * 100))
  const total = Math.floor(centis / 100)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${pad(Math.floor(total / 60))}:${pad(total % 60)}.${pad(centis % 100)}`
}
