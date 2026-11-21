// Microphone capture: getUserMedia with all processing off, an AudioWorklet
// streaming PCM blocks (DESIGN.md "Audio").
// `?worker&url` emits the file as a real asset (small assets are otherwise
// inlined as data: URLs, which not every browser accepts for worklets).
import workletUrl from './capture.worklet.js?worker&url'
import { MAX_TAKE_SECONDS } from './crop'
import { PeakAccumulator } from './peaks'

export interface CaptureStats {
  callbacks: number
  samples: number
  /** Largest absolute sample so far, 0..1. */
  peak: number
  sampleRate: number
  channels: number
}

export class Capture {
  readonly peaks: PeakAccumulator
  readonly stats: CaptureStats
  private chunks: Float32Array[][]
  private frames = 0
  private stopped = false
  private readonly context: AudioContext
  private readonly stream: MediaStream
  private readonly node: AudioWorkletNode
  private readonly onLimit: () => void
  private readonly onActivity: () => void

  private constructor(
    context: AudioContext,
    stream: MediaStream,
    node: AudioWorkletNode,
    channels: number,
    onLimit: () => void,
    onActivity: () => void,
  ) {
    this.context = context
    this.stream = stream
    this.node = node
    this.onLimit = onLimit
    this.onActivity = onActivity
    this.stats = { callbacks: 0, samples: 0, peak: 0, sampleRate: context.sampleRate, channels }
    this.chunks = Array.from({ length: channels }, () => [])
    this.peaks = new PeakAccumulator(context.sampleRate)
    node.port.onmessage = (event: MessageEvent<{ channels: Float32Array[]; callbacks: number }>) => this.onBlock(event.data)
  }

  static async start(deviceId: string, onLimit: () => void, onActivity: () => void): Promise<Capture> {
    const audio: MediaTrackConstraints = { echoCancellation: false, noiseSuppression: false, autoGainControl: false }
    let stream: MediaStream
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: deviceId ? { ...audio, deviceId: { exact: deviceId } } : audio })
    } catch (error) {
      // The remembered device may be gone: fall back to the default.
      if (deviceId && error instanceof DOMException && error.name === 'OverconstrainedError') {
        stream = await navigator.mediaDevices.getUserMedia({ audio })
      } else throw error
    }
    try {
      const track = stream.getAudioTracks()[0]
      const channels = Math.max(1, Math.min(8, track?.getSettings().channelCount ?? 1))
      const context = new AudioContext({ latencyHint: 'interactive' })
      await context.audioWorklet.addModule(workletUrl)
      const source = context.createMediaStreamSource(stream)
      const node = new AudioWorkletNode(context, 'loquaris-capture', {
        numberOfInputs: 1,
        numberOfOutputs: 1,
        channelCount: channels,
        channelCountMode: 'explicit',
        outputChannelCount: [channels],
      })
      const mute = context.createGain()
      mute.gain.value = 0
      source.connect(node).connect(mute).connect(context.destination)
      if (context.state === 'suspended') await context.resume()
      return new Capture(context, stream, node, channels, onLimit, onActivity)
    } catch (error) {
      stream.getTracks().forEach((track) => track.stop())
      throw error
    }
  }

  get elapsed(): number {
    return this.frames / this.context.sampleRate
  }

  private onBlock(data: { channels: Float32Array[]; callbacks: number }) {
    if (this.stopped) return
    this.stats.callbacks = data.callbacks
    const length = data.channels[0]?.length ?? 0
    if (length > 0) {
      const planar: Float32Array[] = []
      for (let c = 0; c < this.stats.channels; c++) {
        const channel = data.channels[c] ?? data.channels[0]
        planar.push(channel)
        this.chunks[c].push(channel)
        for (let i = 0; i < channel.length; i++) {
          const value = Math.abs(channel[i])
          if (value > this.stats.peak) this.stats.peak = value
        }
      }
      this.peaks.push(planar)
      this.frames += length
      this.stats.samples += length * this.stats.channels
    }
    this.onActivity()
    if (this.frames > MAX_TAKE_SECONDS * this.context.sampleRate) {
      this.stopped = true
      this.onLimit()
    }
  }

  /** Stop capturing and return the planar samples. */
  async stop(): Promise<{ channels: Float32Array[]; sampleRate: number }> {
    this.stopped = true
    this.stream.getTracks().forEach((track) => track.stop())
    this.node.port.onmessage = null
    this.node.disconnect()
    const sampleRate = this.context.sampleRate
    await this.context.close().catch(() => {})
    const channels = this.chunks.map((parts) => {
      const merged = new Float32Array(parts.reduce((n, part) => n + part.length, 0))
      let offset = 0
      for (const part of parts) {
        merged.set(part, offset)
        offset += part.length
      }
      return merged
    })
    this.chunks = []
    return { channels, sampleRate }
  }
}
