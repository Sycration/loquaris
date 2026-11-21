import { describe, expect, it } from 'vitest'
import { formatTenths, lessonRecordingSummary, type RecordingInfo } from '../summary'

const clip = (seconds: number, sampleRate = 1000) => ({ blob: 'x', sampleRate, frames: Math.round(seconds * sampleRate), channels: 1 })
const gaps = { gapSameMs: 100, gapDifferentMs: 500 }

describe('lessonRecordingSummary', () => {
  it('is incomplete for an empty lesson or a missing clip', () => {
    expect(lessonRecordingSummary([], gaps)).toBe('Recordings incomplete')
    expect(
      lessonRecordingSummary(
        [
          { participant: 'teacher', audio: clip(1), trim: null },
          { participant: 'student', audio: null, trim: null },
        ],
        gaps,
      ),
    ).toBe('Recordings incomplete')
  })

  it('adds cropped durations and gaps by participant change', () => {
    const entries: RecordingInfo[] = [
      { participant: 'teacher', audio: clip(2), trim: null },
      { participant: 'teacher', audio: clip(3), trim: { start: 500, end: 1500 } },
      { participant: 'student', audio: clip(1.2), trim: null },
    ]
    // 2 + 1 + 1.2 + 0.1 + 0.5 = 4.8
    expect(lessonRecordingSummary(entries, gaps)).toBe('00:04.8')
  })

  it('formats hours', () => {
    expect(formatTenths(59.94)).toBe('00:59.9')
    expect(formatTenths(61.25)).toBe('01:01.3')
    expect(formatTenths(3600)).toBe('1:00:00.0')
    expect(formatTenths(3725.5)).toBe('1:02:05.5')
  })
})
