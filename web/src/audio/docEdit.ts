// Writing an entry's clip and crop: each call is one transaction with the
// local origin and its own undo step (FEATURES §10).
import type { EditorServices } from '../editor/context'
import { asUndoStep } from '../doc/undo'
import { LOCAL_ORIGIN, roots, type AudioRef, type Trim, type YEntry } from '../doc/schema'

/** Replace (or clear, with null) the clip; the crop is cleared. Returns
 * false if the entry no longer exists. */
export function setEntryAudio(services: EditorServices, entryId: string, audio: AudioRef | null): boolean {
  return write(services, entryId, (entry) => {
    entry.set('audio', audio ? { ...audio } : null)
    entry.set('trim', null)
  })
}

export function setEntryTrim(services: EditorServices, entryId: string, trim: Trim | null): boolean {
  return write(services, entryId, (entry) => {
    entry.set('trim', trim ? { start: trim.start, end: trim.end } : null)
  })
}

function write(services: EditorServices, entryId: string, fn: (entry: YEntry) => void): boolean {
  const entry = roots(services.doc).entries.get(entryId)
  if (!entry) return false
  asUndoStep(services.undo, () => services.doc.transact(() => fn(entry), LOCAL_ORIGIN))
  return true
}
