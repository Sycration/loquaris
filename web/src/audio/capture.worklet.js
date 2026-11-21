/* global AudioWorkletProcessor, registerProcessor */
// Streams the input's PCM blocks to the main thread (DESIGN.md "Audio").
// Blocks of 128 frames are batched to about 1024 frames per message. Every
// process() call counts as a callback, even when the input is empty.
class CaptureProcessor extends AudioWorkletProcessor {
  constructor() {
    super()
    this.pending = []
    this.pendingFrames = 0
    this.callbacks = 0
  }

  process(inputs) {
    this.callbacks++
    const input = inputs[0]
    if (input && input.length > 0 && input[0].length > 0) {
      this.pending.push(input.map((channel) => channel.slice()))
      this.pendingFrames += input[0].length
    }
    if (this.pendingFrames >= 1024 || (this.pending.length === 0 && this.callbacks % 8 === 0)) this.flush()
    return true
  }

  flush() {
    const channelCount = this.pending.length > 0 ? this.pending[0].length : 0
    const channels = []
    for (let c = 0; c < channelCount; c++) {
      const merged = new Float32Array(this.pendingFrames)
      let offset = 0
      for (const block of this.pending) {
        merged.set(block[c] ?? block[0], offset)
        offset += block[0].length
      }
      channels.push(merged)
    }
    this.port.postMessage({ channels, callbacks: this.callbacks }, channels.map((channel) => channel.buffer))
    this.pending = []
    this.pendingFrames = 0
  }
}

registerProcessor('loquaris-capture', CaptureProcessor)
