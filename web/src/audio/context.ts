// One shared AudioContext for playback (recorder preview and lessons).
let shared: AudioContext | null = null

export function playbackContext(): AudioContext {
  if (!shared || shared.state === 'closed') shared = new AudioContext({ latencyHint: 'interactive' })
  if (shared.state === 'suspended') void shared.resume()
  return shared
}
