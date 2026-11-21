import { useSyncExternalStore } from 'react'

let open = false
const listeners = new Set<() => void>()

export function setHistoryOpen(value: boolean): void {
  open = value
  listeners.forEach((l) => l())
}

export function useHistoryOpen(): boolean {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => open,
  )
}
