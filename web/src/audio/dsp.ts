// Take processing (FEATURES §19): 10 Hz high-pass per channel to remove DC,
// then linear fades in and out. Plain functions on planar float channels.

export const HIGH_PASS_HZ = 10
export const FADE_SECONDS = 0.25

/** One-pole high-pass filter, in place. */
export function highPass(samples: Float32Array, sampleRate: number, cutoffHz = HIGH_PASS_HZ): void {
  if (samples.length === 0 || sampleRate <= 0) return
  const rc = 1 / (2 * Math.PI * cutoffHz)
  const dt = 1 / sampleRate
  const alpha = rc / (rc + dt)
  let previousInput = samples[0]
  let previousOutput = 0
  samples[0] = 0
  for (let i = 1; i < samples.length; i++) {
    const input = samples[i]
    previousOutput = alpha * (previousOutput + input - previousInput)
    previousInput = input
    samples[i] = previousOutput
  }
}

/** Linear fade in and out, in place. The fades overlap on short clips, where
 * each sample is scaled by both. */
export function applyFades(samples: Float32Array, sampleRate: number, seconds = FADE_SECONDS): void {
  const length = samples.length
  const fade = Math.min(length, Math.round(seconds * sampleRate))
  if (fade <= 0) return
  for (let i = 0; i < fade; i++) {
    const gain = i / fade
    samples[i] *= gain
    samples[length - 1 - i] *= gain
  }
}

/** Process a finished take: high-pass then fades, on copies. */
export function processTake(channels: readonly Float32Array[], sampleRate: number): Float32Array[] {
  return channels.map((channel) => {
    const copy = new Float32Array(channel)
    highPass(copy, sampleRate)
    applyFades(copy, sampleRate)
    return copy
  })
}

/** Largest absolute sample across the channels, 0..1+. */
export function peakAmplitude(channels: readonly Float32Array[]): number {
  let peak = 0
  for (const channel of channels) {
    for (let i = 0; i < channel.length; i++) {
      const value = Math.abs(channel[i])
      if (value > peak) peak = value
    }
  }
  return peak
}
