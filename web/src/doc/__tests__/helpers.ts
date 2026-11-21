import * as Y from 'yjs'
import { addEntry, addLesson, readStructure } from '../ops'
import { entryLanguage, entryParticipant, entryTextString, roots, type Language, type Participant } from '../schema'

/** Build a document from a compact description: one array per lesson, each
 * entry "t:text" (teacher L1), "T:text" (teacher Latin) or "s:text"
 * (student). Returns the doc and the entry IDs by text. */
export function buildDoc(lessons: string[][], doc = new Y.Doc()): { doc: Y.Doc; ids: Record<string, string>; lessonIds: string[] } {
  const ids: Record<string, string> = {}
  const lessonIds: string[] = []
  for (const entries of lessons) {
    const lesson = addLesson(doc)
    lessonIds.push(lesson)
    let after: string | null = null
    for (const spec of entries) {
      const [kind, text] = [spec[0], spec.slice(2)]
      const participant: Participant = kind === 's' ? 'student' : 'teacher'
      const language: Language = kind === 't' ? 'l1' : 'latin'
      after = addEntry(doc, { lesson, after, participant, language, text })!
      ids[text] = after
    }
  }
  return { doc, ids, lessonIds }
}

/** The script as texts per lesson. */
export function texts(doc: Y.Doc): string[][] {
  const { entries } = roots(doc)
  return readStructure(doc).lessons.map((lesson) => lesson.entryIds.map((id) => entryTextString(entries.get(id)!)))
}

/** The script as "kind:text" per lesson, kind as in buildDoc. */
export function kinds(doc: Y.Doc): string[][] {
  const { entries } = roots(doc)
  return readStructure(doc).lessons.map((lesson) =>
    lesson.entryIds.map((id) => {
      const entry = entries.get(id)!
      const kind = entryParticipant(entry) === 'student' ? 's' : entryLanguage(entry) === 'latin' ? 'T' : 't'
      return `${kind}:${entryTextString(entry)}`
    }),
  )
}

/** Connect two docs so every update is delivered immediately (or queued
 * while `paused`). */
export function connect(a: Y.Doc, b: Y.Doc) {
  const queue: [Y.Doc, Uint8Array][] = []
  let paused = false
  const relay = (target: Y.Doc) => (update: Uint8Array, origin: unknown) => {
    if (origin === 'remote') return
    if (paused) queue.push([target, update])
    else Y.applyUpdate(target, update, 'remote')
  }
  a.on('update', relay(b))
  b.on('update', relay(a))
  return {
    pause() {
      paused = true
    },
    resume() {
      paused = false
      for (const [target, update] of queue.splice(0)) Y.applyUpdate(target, update, 'remote')
    },
  }
}

export function clone(doc: Y.Doc): Y.Doc {
  const copy = new Y.Doc()
  Y.applyUpdate(copy, Y.encodeStateAsUpdate(doc), 'remote')
  return copy
}
