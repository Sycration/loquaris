// The chosen audio input is remembered per browser.
const KEY = 'loquaris.audioInput'

/** '' is the system default. */
export function getSavedDevice(): string {
  try {
    return localStorage.getItem(KEY) ?? ''
  } catch {
    return ''
  }
}

export function saveDevice(id: string): void {
  try {
    if (id) localStorage.setItem(KEY, id)
    else localStorage.removeItem(KEY)
  } catch {
    // Storage may be unavailable; the choice just isn't remembered.
  }
}

export interface InputDevice {
  id: string
  label: string
}

/** The audio inputs, "System default" first. Labels are generic until the
 * browser has granted microphone permission. */
export async function listInputs(): Promise<InputDevice[]> {
  const result: InputDevice[] = [{ id: '', label: 'System default' }]
  try {
    const devices = await navigator.mediaDevices.enumerateDevices()
    let n = 0
    for (const device of devices) {
      if (device.kind !== 'audioinput' || device.deviceId === 'default' || device.deviceId === '') continue
      n++
      result.push({ id: device.deviceId, label: device.label || `Microphone ${n}` })
    }
  } catch {
    // No device access; only the default is offered.
  }
  return result
}
