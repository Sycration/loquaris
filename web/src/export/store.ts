// Open state of the export dialog, shared by the menu item and the mount.
import { useSyncExternalStore } from 'react'

let open = false
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

export function setExportOpen(value: boolean): void {
  open = value
  emit()
}

export function useExportOpen(): boolean {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => listeners.delete(l)
    },
    () => open,
  )
}
