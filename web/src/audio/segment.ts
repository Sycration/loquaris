// Play a frame range of a clip with the 10 ms ramps at crop edges that cut
// into the clip (FEATURES §11).
import { rampEnvelope, type Ramps } from './crop'

export interface Segment {
  stop: () => void
}

/** Start playing frames [startFrame, endFrame) of `buffer` at context time
 * `when`. `onEnded` runs when it finishes by itself, not after stop(). */
export function playSegment(
  context: AudioContext,
  buffer: AudioBuffer,
  startFrame: number,
  endFrame: number,
  ramps: Ramps,
  when: number,
  onEnded?: () => void,
): Segment {
  const source = context.createBufferSource()
  source.buffer = buffer
  const gain = context.createGain()
  const duration = Math.max(0, (endFrame - startFrame) / buffer.sampleRate)
  const envelope = rampEnvelope(duration, ramps)
  gain.gain.setValueAtTime(envelope[0][1], when)
  for (let i = 1; i < envelope.length; i++) gain.gain.linearRampToValueAtTime(envelope[i][1], when + envelope[i][0])
  source.connect(gain).connect(context.destination)
  let stopped = false
  source.onended = () => {
    source.disconnect()
    gain.disconnect()
    if (!stopped) onEnded?.()
  }
  source.start(when, startFrame / buffer.sampleRate, duration)
  return {
    stop: () => {
      if (stopped) return
      stopped = true
      try {
        source.stop()
      } catch {
        // Already ended.
      }
    },
  }
}
