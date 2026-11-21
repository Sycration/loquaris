// The recorder's waveform (FEATURES §11): live while recording, otherwise
// the whole clip with the outside of the crop darkened, crop handles, and a
// playhead. Peaks are drawn on a canvas; handles and playhead are DOM.
import { useEffect, useLayoutEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { columnsFor, halfScales } from './peaks'
import { formatClock } from './crop'
import type { LoadedClip } from './clips'
import type { PeakAccumulator } from './peaks'

const LIVE_WINDOW_SECONDS = 10
const BLOCKS = 400
const LABEL_WIDTH = 64

function cssVar(element: Element, name: string, fallback: string): string {
  return getComputedStyle(element).getPropertyValue(name).trim() || fallback
}

export interface WaveformProps {
  mode: 'live' | 'clip' | 'empty'
  live: PeakAccumulator | null
  /** Bumps whenever live data changes. */
  tick: number
  clip: LoadedClip | null
  range: { start: number; end: number }
  cursor: number
  disabled: boolean
  elapsed: number
  onSeek: (frame: number) => void
  onCropCommit: (range: { start: number; end: number }, moving: 'start' | 'end') => void
}

export function Waveform({ mode, live, tick, clip, range, cursor, disabled, elapsed, onSeek, onCropCommit }: WaveformProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(600)
  const [drag, setDrag] = useState<{ moving: 'start' | 'end'; range: { start: number; end: number } } | null>(null)
  const HEIGHT = 120

  useLayoutEffect(() => {
    const box = boxRef.current
    if (!box) return
    const update = () => setWidth(Math.max(100, Math.floor(box.clientWidth)))
    update()
    const observer = new ResizeObserver(update)
    observer.observe(box)
    return () => observer.disconnect()
  }, [])

  const shown = drag?.range ?? range
  const frames = clip?.frames ?? 0
  const xOf = (frame: number) => (frames > 0 ? (frame / frames) * width : 0)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ratio = window.devicePixelRatio || 1
    canvas.width = Math.floor(width * ratio)
    canvas.height = Math.floor(HEIGHT * ratio)
    const g = canvas.getContext('2d')
    if (!g) return
    g.setTransform(ratio, 0, 0, ratio, 0, 0)
    g.clearRect(0, 0, width, HEIGHT)
    const mid = HEIGHT / 2
    const wave = cssVar(canvas, '--text', '#f2f2f0')
    g.fillStyle = cssVar(canvas, '--surface-2', '#1b1d21')
    g.fillRect(0, 0, width, HEIGHT)
    g.fillStyle = cssVar(canvas, '--border-strong', '#41454e')
    g.fillRect(0, mid, width, 1)

    let peaks: ArrayLike<number> | null = null
    let from = 0
    let to = 0
    if (mode === 'live' && live) {
      // Grows to fill a 10 s window, then shrinks to keep the whole take visible.
      peaks = live.peaks
      to = live.blocks
      from = 0
    } else if (mode === 'clip' && clip) {
      peaks = clip.peaks
      to = clip.peaks.length / 2
    }
    if (!peaks || to === 0) return
    const span = mode === 'live' ? Math.max(to, LIVE_WINDOW_SECONDS * BLOCKS) : to
    const drawWidth = mode === 'live' ? Math.max(1, Math.round((to / span) * width)) : width
    const columns = columnsFor(peaks, from, to, drawWidth)
    const scale = halfScales(columns)
    g.fillStyle = wave
    columns.forEach((column, i) => {
      const x = (i / columns.length) * drawWidth
      const w = Math.max(1, drawWidth / columns.length)
      const top = mid - column.max * scale.up * (mid - 2)
      const bottom = mid - column.min * scale.down * (mid - 2)
      g.fillRect(x, top, w, Math.max(1, bottom - top))
    })
    if (mode === 'clip' && clip) {
      g.fillStyle = 'rgb(0 0 0 / 0.55)'
      g.fillRect(0, 0, xOf(shown.start), HEIGHT)
      g.fillRect(xOf(shown.end), 0, width - xOf(shown.end), HEIGHT)
    }
    // xOf closes over frames/width, both in the deps.
  }, [mode, live, tick, clip, width, shown.start, shown.end, frames]) // eslint-disable-line react-hooks/exhaustive-deps

  const frameAt = (clientX: number) => {
    const rect = boxRef.current!.getBoundingClientRect()
    return Math.round(Math.min(1, Math.max(0, (clientX - rect.left) / rect.width)) * frames)
  }

  const onBackgroundDown = (event: ReactPointerEvent) => {
    if (mode !== 'clip' || !clip || disabled || event.button !== 0) return
    event.currentTarget.setPointerCapture(event.pointerId)
    onSeek(frameAt(event.clientX))
  }
  const onBackgroundMove = (event: ReactPointerEvent) => {
    if (event.buttons & 1 && event.currentTarget.hasPointerCapture(event.pointerId)) onSeek(frameAt(event.clientX))
  }

  const startHandle = (moving: 'start' | 'end') => ({
    onPointerDown: (event: ReactPointerEvent) => {
      if (disabled || event.button !== 0) return
      event.stopPropagation()
      event.currentTarget.setPointerCapture(event.pointerId)
      setDrag({ moving, range })
    },
    onPointerMove: (event: ReactPointerEvent) => {
      if (!drag || drag.moving !== moving) return
      const frame = frameAt(event.clientX)
      setDrag({ moving, range: moving === 'start' ? { start: frame, end: range.end } : { start: range.start, end: frame } })
    },
    onPointerUp: (event: ReactPointerEvent) => {
      if (!drag || drag.moving !== moving) return
      const frame = frameAt(event.clientX)
      const final = moving === 'start' ? { start: frame, end: range.end } : { start: range.start, end: frame }
      setDrag(null)
      onCropCommit(final, moving)
      event.currentTarget.releasePointerCapture(event.pointerId)
    },
    onPointerCancel: () => setDrag(null),
  })

  // Times above the handles, pushed apart when close.
  let startLabel = 0
  let endLabel = 0
  if (clip) {
    startLabel = xOf(shown.start) - LABEL_WIDTH
    endLabel = xOf(shown.end)
    const overlap = startLabel + LABEL_WIDTH - endLabel + 4
    if (overlap > 0) {
      startLabel -= overlap / 2
      endLabel += overlap / 2
    }
    startLabel = Math.max(0, Math.min(startLabel, width - 2 * LABEL_WIDTH))
    endLabel = Math.max(startLabel + LABEL_WIDTH, Math.min(endLabel, width - LABEL_WIDTH))
  }
  const rate = clip?.sampleRate ?? 1
  const playheadX = xOf(cursor)
  const playLabel = Math.max(0, Math.min(playheadX - LABEL_WIDTH / 2, width - LABEL_WIDTH))

  return (
    <div className="waveform" style={{ height: HEIGHT + 44 }}>
      <div className="waveform__box" ref={boxRef} style={{ height: HEIGHT }}>
        <canvas ref={canvasRef} className="waveform__canvas" style={{ width, height: HEIGHT }} />
        {mode === 'empty' && <div className="waveform__empty">No clip yet. Record, or import a WAV file.</div>}
        {mode === 'live' && <div className="waveform__elapsed">{formatClock(elapsed)}</div>}
        {mode === 'clip' && clip && (
          <>
            <div className="waveform__seek" onPointerDown={onBackgroundDown} onPointerMove={onBackgroundMove} />
            <div
              className="waveform__handle waveform__handle--start"
              style={{ left: xOf(shown.start) - 10 }}
              role="slider"
              aria-label="Clip start"
              aria-valuemin={0}
              aria-valuemax={clip.frames / rate}
              aria-valuenow={shown.start / rate}
              title="Drag to choose where the clip starts"
              {...startHandle('start')}
            />
            <div
              className="waveform__handle waveform__handle--end"
              style={{ left: xOf(shown.end) }}
              role="slider"
              aria-label="Clip end"
              aria-valuemin={0}
              aria-valuemax={clip.frames / rate}
              aria-valuenow={shown.end / rate}
              title="Drag to choose where the clip ends"
              {...startHandle('end')}
            />
            <div className="waveform__playhead" style={{ left: playheadX }} />
          </>
        )}
      </div>
      {mode === 'clip' && clip && (
        <>
          <div className="waveform__time waveform__time--top" style={{ left: startLabel, width: LABEL_WIDTH, textAlign: 'right' }}>
            {formatClock(shown.start / rate)}
          </div>
          <div className="waveform__time waveform__time--top" style={{ left: endLabel, width: LABEL_WIDTH }}>
            {formatClock(shown.end / rate)}
          </div>
          <div className="waveform__time waveform__time--bottom waveform__time--playhead" style={{ left: playLabel, width: LABEL_WIDTH, textAlign: 'center' }}>
            {formatClock(cursor / rate)}
          </div>
        </>
      )}
    </div>
  )
}
