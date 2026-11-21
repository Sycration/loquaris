// Per-user undo (DESIGN.md, "Undo"): tracks only this client's own
// transactions, groups typing within 500 ms, and covers the three roots.
import * as Y from 'yjs'
import { LOCAL_ORIGIN, roots } from './schema'

export const CAPTURE_TIMEOUT_MS = 500

export function createUndoManager(doc: Y.Doc): Y.UndoManager {
  const { meta, lessons, entries } = roots(doc)
  return new Y.UndoManager([meta, lessons, entries], {
    trackedOrigins: new Set([LOCAL_ORIGIN]),
    captureTimeout: CAPTURE_TIMEOUT_MS,
  })
}

/** Run a structural change as its own undo step, never merged with typing
 * before or after it. */
export function asUndoStep<T>(undo: Y.UndoManager | null, fn: () => T): T {
  undo?.stopCapturing()
  try {
    return fn()
  } finally {
    undo?.stopCapturing()
  }
}
