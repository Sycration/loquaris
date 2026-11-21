// Tracks open dialogs and menus, so global shortcuts can stand down while one
// is open (FEATURES §14: main shortcuts are off while a dialog is open or a
// key is being captured), and so only the topmost layer handles Escape.
import { useEffect, useId, useSyncExternalStore } from 'react'

export interface OverlayLayer {
  id: string
  kind: 'dialog' | 'menu' | 'capture'
  /** Dialogs that edit the document (e.g. project details) let undo/redo
   * through to the document. */
  allowHistory?: boolean
}

let layers: readonly OverlayLayer[] = []
const listeners = new Set<() => void>()

function emit() {
  for (const listener of listeners) listener()
}

export function pushLayer(layer: OverlayLayer): () => void {
  layers = [...layers, layer]
  emit()
  return () => {
    layers = layers.filter((other) => other.id !== layer.id)
    emit()
  }
}

export function getLayers(): readonly OverlayLayer[] {
  return layers
}

export function topLayerId(): string | null {
  return layers.length ? layers[layers.length - 1].id : null
}

export function isTopLayer(id: string): boolean {
  return topLayerId() === id
}

export function useLayers(): readonly OverlayLayer[] {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },
    getLayers,
  )
}

/** Register a layer while `active`; returns its id. */
export function useOverlayLayer(active: boolean, kind: OverlayLayer['kind'], allowHistory = false): string {
  const id = useId()
  useEffect(() => {
    if (!active) return
    return pushLayer({ id, kind, allowHistory })
  }, [active, id, kind, allowHistory])
  return id
}
