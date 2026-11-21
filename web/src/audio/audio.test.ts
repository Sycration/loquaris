import { describe, expect, it } from 'vitest'
import { clampCrop, effectiveRange, formatClock, gainAt, normalizeTrim, playbackStart, rampEnvelope, rampsFor, seek } from './crop'
import { applyFades, highPass, peakAmplitude, processTake } from './dsp'
import { columnsFor, computePeaks, halfScales, PeakAccumulator } from './peaks'
import { planPlayback, gapSeconds } from './schedule'
import { encodeWav, floatToInt16, parseWavHeader } from './wav'

const clip = { frames: 48000, sampleRate: 48000 }

describe('wav', () => {
  it('encodes 16-bit PCM and parses back', () => {
    const left = Float32Array.from([0, 1, -1, 0.5])
    const right = Float32Array.from([0, -1, 1, -0.5])
    const bytes = encodeWav([left, right], 44100)
    expect(bytes.length).toBe(44 + 16)
    const header = parseWavHeader(bytes)
    expect(header).toEqual({ format: 1, channels: 2, sampleRate: 44100, bitsPerSample: 16, frames: 4 })
    const view = new DataView(bytes.buffer)
    expect(view.getInt16(44 + 4, true)).toBe(32767)
    expect(view.getInt16(44 + 6, true)).toBe(-32768)
  })
  it('clamps and rejects non-wav', () => {
    expect(floatToInt16(2)).toBe(32767)
    expect(floatToInt16(-2)).toBe(-32768)
    expect(parseWavHeader(new Uint8Array(100))).toBeNull()
    expect(parseWavHeader(new Uint8Array(4))).toBeNull()
  })
})

describe('dsp', () => {
  it('high-pass removes DC', () => {
    const data = new Float32Array(48000).fill(0.5)
    highPass(data, 48000)
    expect(Math.abs(data[data.length - 1])).toBeLessThan(0.01)
  })
  it('fades linearly and overlaps on short clips', () => {
    const data = new Float32Array(1000).fill(1)
    applyFades(data, 1000, 0.25)
    expect(data[0]).toBe(0)
    expect(data[125]).toBeCloseTo(0.5)
    expect(data[500]).toBe(1)
    expect(data[999]).toBe(0)
    const short = new Float32Array(100).fill(1)
    applyFades(short, 1000, 0.25)
    expect(short[50]).toBeGreaterThan(0)
    expect(Math.max(...short)).toBeLessThan(1)
  })
  it('processTake copies and reports peaks', () => {
    const input = [new Float32Array(100).fill(0.2)]
    const out = processTake(input, 1000)
    expect(input[0][50]).toBeCloseTo(0.2)
    expect(out[0][0]).toBe(0)
    expect(peakAmplitude([Float32Array.from([0.1, -0.7])])).toBeCloseTo(0.7)
  })
})

describe('peaks', () => {
  it('computes min/max per 1/400 s', () => {
    const data = new Float32Array(800)
    data[10] = 0.5
    data[500] = -0.25
    const peaks = computePeaks([data], 400 * 100)
    expect(peaks.length).toBe(Math.ceil(800 / 100) * 2)
    expect(peaks[1]).toBe(0.5)
    expect(peaks[5 * 2]).toBe(-0.25)
  })
  it('accumulator matches batch computation', () => {
    const data = Float32Array.from({ length: 1234 }, (_, i) => Math.sin(i / 7))
    const acc = new PeakAccumulator(4000)
    acc.push([data.subarray(0, 500)])
    acc.push([data.subarray(500)])
    const batch = computePeaks([data], 4000)
    expect(acc.blocks).toBe(Math.floor(1234 / 10))
    for (let i = 0; i < acc.peaks.length; i++) expect(acc.peaks[i]).toBeCloseTo(batch[i])
  })
  it('columns cover at least 5 blocks and scale halves', () => {
    const peaks = new Float32Array(2 * 100)
    for (let i = 0; i < 100; i++) {
      peaks[i * 2] = -0.1
      peaks[i * 2 + 1] = 0.5
    }
    const cols = columnsFor(peaks, 0, 100, 100)
    expect(cols.length).toBe(20)
    const scale = halfScales(cols)
    expect(scale.up).toBeCloseTo(2)
    expect(scale.down).toBeCloseTo(10)
    expect(columnsFor(peaks, 0, 0, 10)).toEqual([])
  })
})

describe('crop', () => {
  it('clamps out-of-range trims', () => {
    expect(effectiveRange(clip, null)).toEqual({ start: 0, end: 48000 })
    expect(effectiveRange(clip, { start: -5, end: 99999 })).toEqual({ start: 0, end: 48000 })
    expect(effectiveRange(clip, { start: 60000, end: 70000 })).toEqual({ start: 47999, end: 48000 })
    expect(effectiveRange(clip, { start: 100, end: 100 })).toEqual({ start: 100, end: 101 })
  })
  it('keeps at least 50 ms', () => {
    expect(clampCrop(clip, { start: 47000, end: 40000 }, 'start')).toEqual({ start: 40000 - 2400, end: 40000 })
    expect(clampCrop(clip, { start: 10000, end: 10100 }, 'end')).toEqual({ start: 10000, end: 12400 })
  })
  it('stores whole-clip crop as null', () => {
    expect(normalizeTrim(clip, { start: 0, end: 48000 })).toBeNull()
    expect(normalizeTrim(clip, { start: 0, end: 47000 })).toEqual({ start: 0, end: 47000 })
  })
  it('playback start and seek', () => {
    const range = { start: 1000, end: 5000 }
    expect(playbackStart(3000, range)).toBe(3000)
    expect(playbackStart(5000, range)).toBe(1000)
    expect(playbackStart(0, range)).toBe(1000)
    expect(seek(3000, 5, clip, range)).toBe(5000)
    expect(seek(3000, -5, clip, range)).toBe(1000)
  })
  it('ramps only where the crop cuts in', () => {
    expect(rampsFor(clip, { start: 0, end: 48000 })).toEqual({ fadeIn: false, fadeOut: false })
    expect(rampsFor(clip, { start: 100, end: 47000 })).toEqual({ fadeIn: true, fadeOut: true })
    expect(rampsFor(clip, { start: 100, end: 48000 }, 500).fadeIn).toBe(false)
    const ramps = { fadeIn: true, fadeOut: true }
    expect(rampEnvelope(1, ramps)).toEqual([[0, 0], [0.01, 1], [0.99, 1], [1, 0]])
    expect(gainAt(0.005, 1, ramps)).toBeCloseTo(0.5)
    expect(gainAt(0.5, 1, ramps)).toBe(1)
    expect(gainAt(0.5, 1, { fadeIn: false, fadeOut: false })).toBe(1)
  })
  it('formats the clock', () => {
    expect(formatClock(65.456)).toBe('01:05.46')
    expect(formatClock(-1)).toBe('00:00.00')
  })
})

describe('schedule', () => {
  const audio = { blob: 'b', sampleRate: 1000, frames: 2000, channels: 1 }
  const gaps = { gapSameMs: 100, gapDifferentMs: 500 }
  const entries = [
    { id: 'a', participant: 'teacher' as const, audio, trim: null },
    { id: 'b', participant: 'teacher' as const, audio, trim: { start: 500, end: 1500 } },
    { id: 'c', participant: 'student' as const, audio, trim: null },
    { id: 'd', participant: 'student' as const, audio: null, trim: null },
  ]
  it('applies gaps by participant', () => {
    expect(gapSeconds('teacher', 'student', gaps)).toBe(0.5)
    const plan = planPlayback(entries, 0, gaps)
    expect(plan.items.map((i) => i.offset)).toEqual([0, 2.1, 3.6])
    expect(plan.items[1].duration).toBe(1)
    expect(plan.stopMessage).toBe('Playback stopped: entry 4 has no recording.')
  })
  it('messages for a starting entry without a clip', () => {
    const plan = planPlayback(entries, 3, gaps)
    expect(plan.items).toEqual([])
    expect(plan.stopMessage).toBe('Entry 4 has no recording.')
    expect(planPlayback(entries.slice(0, 3), 1, gaps).stopMessage).toBeNull()
  })
})
