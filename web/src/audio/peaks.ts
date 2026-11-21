// Waveform peaks (FEATURES §11): min/max across channels per 1/400 s block.

export const BLOCKS_PER_SECOND = 400
export const MIN_BLOCKS_PER_COLUMN = 5

/** Interleaved min,max pairs, one per block. */
export function computePeaks(channels: readonly Float32Array[], sampleRate: number): Float32Array {
  const frames = channels[0]?.length ?? 0
  const blockFrames = Math.max(1, Math.round(sampleRate / BLOCKS_PER_SECOND))
  const blocks = Math.ceil(frames / blockFrames)
  const peaks = new Float32Array(blocks * 2)
  for (let block = 0; block < blocks; block++) {
    const start = block * blockFrames
    const end = Math.min(frames, start + blockFrames)
    let min = Infinity
    let max = -Infinity
    for (const channel of channels) {
      for (let i = start; i < end; i++) {
        const value = channel[i]
        if (value < min) min = value
        if (value > max) max = value
      }
    }
    peaks[block * 2] = min === Infinity ? 0 : min
    peaks[block * 2 + 1] = max === -Infinity ? 0 : max
  }
  return peaks
}

/** Incremental peak builder for a live take: feed blocks of mono-or-more
 * channels as they arrive. */
export class PeakAccumulator {
  private readonly blockFrames: number
  private min = Infinity
  private max = -Infinity
  private filled = 0
  readonly peaks: number[] = []

  constructor(sampleRate: number) {
    this.blockFrames = Math.max(1, Math.round(sampleRate / BLOCKS_PER_SECOND))
  }

  push(channels: readonly Float32Array[]): void {
    const frames = channels[0]?.length ?? 0
    for (let i = 0; i < frames; i++) {
      for (const channel of channels) {
        const value = channel[i]
        if (value < this.min) this.min = value
        if (value > this.max) this.max = value
      }
      if (++this.filled === this.blockFrames) this.flush()
    }
  }

  private flush() {
    this.peaks.push(this.min === Infinity ? 0 : this.min, this.max === -Infinity ? 0 : this.max)
    this.min = Infinity
    this.max = -Infinity
    this.filled = 0
  }

  get blocks(): number {
    return this.peaks.length / 2
  }
}

export interface Column {
  min: number
  max: number
}

/** Reduce the blocks in [fromBlock, toBlock) to `width` screen columns, each
 * covering at least MIN_BLOCKS_PER_COLUMN blocks (fewer columns if needed). */
export function columnsFor(peaks: ArrayLike<number>, fromBlock: number, toBlock: number, width: number): Column[] {
  const total = Math.max(0, toBlock - fromBlock)
  if (total === 0 || width <= 0) return []
  const perColumn = Math.max(MIN_BLOCKS_PER_COLUMN, total / width)
  const count = Math.max(1, Math.min(width, Math.floor(total / perColumn)))
  const columns: Column[] = []
  for (let c = 0; c < count; c++) {
    const a = fromBlock + Math.floor((c * total) / count)
    const b = Math.max(a + 1, fromBlock + Math.floor(((c + 1) * total) / count))
    let min = Infinity
    let max = -Infinity
    for (let block = a; block < b && block * 2 + 1 < peaks.length; block++) {
      if (peaks[block * 2] < min) min = peaks[block * 2]
      if (peaks[block * 2 + 1] > max) max = peaks[block * 2 + 1]
    }
    columns.push({ min: min === Infinity ? 0 : min, max: max === -Infinity ? 0 : max })
  }
  return columns
}

/** Top and bottom halves are stretched independently so quiet clips remain
 * visible (display only). Returns the scale for each half. */
export function halfScales(columns: readonly Column[]): { up: number; down: number } {
  let up = 0
  let down = 0
  for (const column of columns) {
    if (column.max > up) up = column.max
    if (-column.min > down) down = -column.min
  }
  return { up: up > 1e-6 ? 1 / up : 1, down: down > 1e-6 ? 1 / down : 1 }
}
