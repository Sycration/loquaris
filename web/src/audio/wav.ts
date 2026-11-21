// WAV helpers: 16-bit PCM encoding of recorded takes and a cheap header
// check for imports (the server fully decodes every upload to validate it).

export const MAX_IMPORT_BYTES = 100 * 1024 * 1024

/** Float samples in [-1, 1] to a clamped 16-bit value. */
export function floatToInt16(sample: number): number {
  const clamped = sample < -1 ? -1 : sample > 1 ? 1 : sample
  return clamped < 0 ? Math.round(clamped * 0x8000) : Math.round(clamped * 0x7fff)
}

function writeAscii(view: DataView, offset: number, text: string) {
  for (let i = 0; i < text.length; i++) view.setUint8(offset + i, text.charCodeAt(i))
}

/** Encode planar float channels as an interleaved 16-bit PCM WAV file. */
export function encodeWav(channels: readonly Float32Array[], sampleRate: number): Uint8Array<ArrayBuffer> {
  const channelCount = channels.length
  const frames = channelCount === 0 ? 0 : channels[0].length
  const dataBytes = frames * channelCount * 2
  const buffer = new ArrayBuffer(44 + dataBytes)
  const view = new DataView(buffer)
  writeAscii(view, 0, 'RIFF')
  view.setUint32(4, 36 + dataBytes, true)
  writeAscii(view, 8, 'WAVE')
  writeAscii(view, 12, 'fmt ')
  view.setUint32(16, 16, true)
  view.setUint16(20, 1, true) // PCM
  view.setUint16(22, channelCount, true)
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, sampleRate * channelCount * 2, true)
  view.setUint16(32, channelCount * 2, true)
  view.setUint16(34, 16, true)
  writeAscii(view, 36, 'data')
  view.setUint32(40, dataBytes, true)
  let offset = 44
  for (let frame = 0; frame < frames; frame++) {
    for (let channel = 0; channel < channelCount; channel++) {
      view.setInt16(offset, floatToInt16(channels[channel][frame]), true)
      offset += 2
    }
  }
  return new Uint8Array(buffer)
}

export interface WavHeader {
  channels: number
  sampleRate: number
  bitsPerSample: number
  /** 1 = PCM, 3 = float, 0xfffe = extensible. */
  format: number
  frames: number
}

/** Parse the fmt and data chunks of a RIFF/WAVE file, or null if it isn't one. */
export function parseWavHeader(bytes: Uint8Array): WavHeader | null {
  if (bytes.length < 12) return null
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  const tag = (offset: number) => String.fromCharCode(...bytes.subarray(offset, offset + 4))
  if (tag(0) !== 'RIFF' || tag(8) !== 'WAVE') return null
  let fmt: Omit<WavHeader, 'frames'> | null = null
  let offset = 12
  while (offset + 8 <= bytes.length) {
    const id = tag(offset)
    const size = view.getUint32(offset + 4, true)
    const body = offset + 8
    if (id === 'fmt ' && size >= 16 && body + 16 <= bytes.length) {
      fmt = {
        format: view.getUint16(body, true),
        channels: view.getUint16(body + 2, true),
        sampleRate: view.getUint32(body + 4, true),
        bitsPerSample: view.getUint16(body + 14, true),
      }
    } else if (id === 'data') {
      if (!fmt || fmt.channels === 0 || fmt.bitsPerSample === 0) return null
      const available = Math.min(size, bytes.length - body)
      const frameBytes = fmt.channels * Math.ceil(fmt.bitsPerSample / 8)
      return { ...fmt, frames: Math.floor(available / frameBytes) }
    }
    offset = body + size + (size % 2)
  }
  return null
}
