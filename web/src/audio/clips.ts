// Decoded clips, cached per blob: the AudioBuffer for playback and the
// waveform peaks (FEATURES §11). Clips are immutable, so entries never go
// stale.
import { urls } from '../api/client'
import type { AudioRef } from '../doc/schema'
import { computePeaks } from './peaks'

export interface LoadedClip {
  buffer: AudioBuffer
  /** Interleaved min,max pairs per 1/400 s. */
  peaks: Float32Array
  sampleRate: number
  frames: number
}

const cache = new Map<string, Promise<LoadedClip>>()

function toClip(buffer: AudioBuffer): LoadedClip {
  const channels = Array.from({ length: buffer.numberOfChannels }, (_, i) => buffer.getChannelData(i))
  return { buffer, peaks: computePeaks(channels, buffer.sampleRate), sampleRate: buffer.sampleRate, frames: buffer.length }
}

export function loadClip(audio: AudioRef): Promise<LoadedClip> {
  let promise = cache.get(audio.blob)
  if (!promise) {
    promise = (async () => {
      const response = await fetch(urls.audio(audio.blob), { credentials: 'same-origin' })
      if (!response.ok) throw new Error(`Could not load the recording (${response.status}).`)
      const bytes = await response.arrayBuffer()
      // Decode at the clip's own rate so trim frames line up.
      const rate = Math.min(768000, Math.max(8000, audio.sampleRate || 48000))
      const context = new OfflineAudioContext(Math.max(1, audio.channels), 1, rate)
      return toClip(await context.decodeAudioData(bytes))
    })()
    cache.set(audio.blob, promise)
    promise.catch(() => cache.delete(audio.blob))
  }
  return promise
}

/** Seed the cache with a take we just encoded and uploaded. */
export function primeClip(blob: string, channels: readonly Float32Array[], sampleRate: number): void {
  try {
    const buffer = new AudioBuffer({ length: channels[0].length, numberOfChannels: channels.length, sampleRate })
    channels.forEach((channel, i) => buffer.copyToChannel(channel as Float32Array<ArrayBuffer>, i))
    cache.set(blob, Promise.resolve(toClip(buffer)))
  } catch {
    // Fall back to fetching it.
  }
}
