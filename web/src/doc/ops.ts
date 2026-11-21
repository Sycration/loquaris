// Editing operations on the project document (FEATURES §5–7, DESIGN.md
// "Changes from the desktop app"). Each operation is one Y transaction with
// the local origin, so it is one undo step and reaches collaborators
// atomically. Operations take IDs, never positions, and re-read the document
// at the start, so they act on the current state including remote changes.
import * as Y from 'yjs'
import { planOrder, sortOrdered, type PlanItem } from './order'
import {
  createEntry,
  createLesson,
  entryAudio,
  entryLanguage,
  entryLemmaChoices,
  entryLesson,
  entryOrder,
  entryParticipant,
  entryTextString,
  entryTrim,
  lessonNameString,
  lessonOrder,
  LOCAL_ORIGIN,
  MAX_GAP_MS,
  randomId,
  roots,
  SYSTEM_ORIGIN,
  type Language,
  type Meta,
  type Participant,
  type YEntry,
} from './schema'

export interface StructureLesson {
  id: string
  entryIds: string[]
}

export interface Structure {
  lessons: StructureLesson[]
  /** Every shown entry ID in script order. */
  flat: string[]
  /** Entries whose lesson no longer exists (a concurrent delete). */
  orphans: string[]
}

/** The ordered lessons and entries of a document. */
export function readStructure(doc: Y.Doc): Structure {
  const { lessons, entries } = roots(doc)
  const lessonList = sortOrdered(
    Array.from(lessons.entries(), ([id, lesson]) => ({ id, order: lessonOrder(lesson) })),
  )
  const byLesson = new Map<string, { id: string; order: string }[]>()
  for (const lesson of lessonList) byLesson.set(lesson.id, [])
  const orphans: string[] = []
  entries.forEach((entry, id) => {
    const bucket = byLesson.get(entryLesson(entry))
    if (bucket) bucket.push({ id, order: entryOrder(entry) })
    else orphans.push(id)
  })
  const result: StructureLesson[] = lessonList.map((lesson) => ({
    id: lesson.id,
    entryIds: sortOrdered(byLesson.get(lesson.id)!).map((entry) => entry.id),
  }))
  return { lessons: result, flat: result.flatMap((lesson) => lesson.entryIds), orphans }
}

function transact<T>(doc: Y.Doc, fn: () => T): T {
  let result!: T
  doc.transact(() => {
    result = fn()
  }, LOCAL_ORIGIN)
  return result
}

/** Rewrite `order` (and `lesson`) so each lesson in `layout` holds exactly
 * the given entries in the given order, touching as few entries as possible. */
function applyLayout(doc: Y.Doc, layout: StructureLesson[], prefer: ReadonlySet<string> = new Set()) {
  const { entries } = roots(doc)
  for (const lesson of layout) {
    const sequence: PlanItem[] = lesson.entryIds.map((id) => {
      const entry = entries.get(id)!
      return { id, order: entryLesson(entry) === lesson.id ? entryOrder(entry) : null }
    })
    const keys = planOrder(sequence, prefer)
    for (const id of lesson.entryIds) {
      const entry = entries.get(id)!
      if (entryLesson(entry) !== lesson.id) entry.set('lesson', lesson.id)
      const key = keys.get(id)
      if (key !== undefined) entry.set('order', key)
    }
  }
}

function cloneLayout(structure: Structure): StructureLesson[] {
  return structure.lessons.map((lesson) => ({ id: lesson.id, entryIds: [...lesson.entryIds] }))
}

// ---- lessons ---------------------------------------------------------------

/** Insert an empty lesson after `after` (null: at the end). Returns its ID. */
export function addLesson(doc: Y.Doc, after: string | null = null, name = ''): string {
  return transact(doc, () => {
    const id = randomId()
    const order = lessonOrderFor(doc, id, after)
    roots(doc).lessons.set(id, createLesson(order, name))
    return id
  })
}

/** The key placing lesson `id` right after `after` (null: at the end),
 * re-keying neighbours if needed. Must run inside a transaction. */
function lessonOrderFor(doc: Y.Doc, id: string, after: string | null): string {
  const { lessons } = roots(doc)
  const ids = readStructure(doc).lessons.map((lesson) => lesson.id).filter((other) => other !== id)
  const index = after === null ? ids.length : ids.indexOf(after) + 1
  ids.splice(index < 0 ? ids.length : index, 0, id)
  const keys = planOrder(
    ids.map((other) => ({ id: other, order: other === id ? null : lessonOrder(lessons.get(other)!) })),
    new Set([id]),
  )
  for (const [other, key] of keys) if (other !== id) lessons.get(other)!.set('order', key)
  return keys.get(id)!
}

/** Delete a lesson and its entries. The last lesson cannot be deleted. */
export function deleteLesson(doc: Y.Doc, id: string): boolean {
  const { lessons, entries } = roots(doc)
  if (!lessons.has(id) || lessons.size <= 1) return false
  transact(doc, () => {
    for (const [entryId, entry] of Array.from(entries.entries())) {
      if (entryLesson(entry) === id) entries.delete(entryId)
    }
    lessons.delete(id)
  })
  return true
}

export function moveLesson(doc: Y.Doc, id: string, direction: -1 | 1): boolean {
  const { lessons } = roots(doc)
  const ids = readStructure(doc).lessons.map((lesson) => lesson.id)
  const index = ids.indexOf(id)
  const target = index + direction
  if (index < 0 || target < 0 || target >= ids.length) return false
  transact(doc, () => {
    ids.splice(index, 1)
    ids.splice(target, 0, id)
    const keys = planOrder(
      ids.map((other) => ({ id: other, order: lessonOrder(lessons.get(other)!) })),
      new Set([id]),
    )
    for (const [other, key] of keys) lessons.get(other)!.set('order', key)
  })
  return true
}

/** Copy a lesson with all its entries right after it. Returns the copy's ID. */
export function duplicateLesson(doc: Y.Doc, id: string): string | null {
  const { lessons, entries } = roots(doc)
  const source = lessons.get(id)
  if (!source) return null
  const entryIds = readStructure(doc).lessons.find((lesson) => lesson.id === id)?.entryIds ?? []
  return transact(doc, () => {
    const copyId = randomId()
    const order = lessonOrderFor(doc, copyId, id)
    lessons.set(copyId, createLesson(order, lessonNameString(source)))
    const keys = planOrder(entryIds.map((entryId) => ({ id: entryId, order: null })))
    for (const entryId of entryIds) {
      entries.set(randomId(), copyEntry(entries.get(entryId)!, copyId, keys.get(entryId)!))
    }
    return copyId
  })
}

// ---- entries ---------------------------------------------------------------

function copyEntry(entry: YEntry, lesson: string, order: string): YEntry {
  return createEntry({
    lesson,
    order,
    participant: entryParticipant(entry),
    language: entryLanguage(entry),
    text: entryTextString(entry),
    audio: entryAudio(entry),
    trim: entryTrim(entry),
    lemmas: entryLemmaChoices(entry),
  })
}

export interface AddEntryOptions {
  lesson: string
  /** Insert after this entry (it must be in `lesson`); null: at the end. */
  after: string | null
  participant: Participant
  language: Language
  text?: string
}

/** Insert a blank entry. Students only speak Latin. Returns its ID. */
export function addEntry(doc: Y.Doc, options: AddEntryOptions): string | null {
  const { lessons, entries } = roots(doc)
  if (!lessons.has(options.lesson)) return null
  return transact(doc, () => {
    const layout = cloneLayout(readStructure(doc))
    const lesson = layout.find((item) => item.id === options.lesson)!
    const id = randomId()
    const afterIndex = options.after === null ? -1 : lesson.entryIds.indexOf(options.after)
    const index = afterIndex < 0 ? lesson.entryIds.length : afterIndex + 1
    lesson.entryIds.splice(index, 0, id)
    const keys = planOrder(
      lesson.entryIds.map((other) => ({ id: other, order: other === id ? null : entryOrder(entries.get(other)!) })),
    )
    for (const [other, key] of keys) if (other !== id) entries.get(other)!.set('order', key)
    entries.set(
      id,
      createEntry({
        lesson: options.lesson,
        order: keys.get(id)!,
        participant: options.participant,
        language: options.participant === 'student' ? 'latin' : options.language,
        text: options.text,
      }),
    )
    return id
  })
}

/** Insert a copy (with audio, crop and lemma choices) after each target.
 * Returns the copies' IDs keyed by original, in script order. */
export function duplicateEntries(doc: Y.Doc, ids: Iterable<string>): Map<string, string> {
  const { entries } = roots(doc)
  const chosen = new Set(Array.from(ids).filter((id) => entries.has(id)))
  const copies = new Map<string, string>()
  if (chosen.size === 0) return copies
  transact(doc, () => {
    for (const lesson of readStructure(doc).lessons) {
      const lessonCopies = new Map<string, string>()
      const next: string[] = []
      for (const id of lesson.entryIds) {
        next.push(id)
        if (chosen.has(id)) {
          const copyId = randomId()
          lessonCopies.set(copyId, id)
          copies.set(id, copyId)
          next.push(copyId)
        }
      }
      if (lessonCopies.size === 0) continue
      const keys = planOrder(
        next.map((id) => ({ id, order: lessonCopies.has(id) ? null : entryOrder(entries.get(id)!) })),
      )
      for (const id of lesson.entryIds) {
        const key = keys.get(id)
        if (key !== undefined) entries.get(id)!.set('order', key)
      }
      for (const [copyId, original] of lessonCopies) {
        entries.set(copyId, copyEntry(entries.get(original)!, lesson.id, keys.get(copyId)!))
      }
    }
  })
  return copies
}

export function deleteEntries(doc: Y.Doc, ids: Iterable<string>): number {
  const { entries } = roots(doc)
  const existing = Array.from(new Set(ids)).filter((id) => entries.has(id))
  if (existing.length === 0) return 0
  transact(doc, () => {
    for (const id of existing) entries.delete(id)
  })
  return existing.length
}

export type KindChange = 'toStudent' | 'toTeacher' | 'toL1' | 'toLatin' | 'toggleLanguage'

/** Change participant or language, keeping the rules: teacher → student
 * forces Latin; student → teacher stays Latin; only teachers switch language.
 * Returns the number of entries changed. */
export function changeKind(doc: Y.Doc, ids: Iterable<string>, change: KindChange): number {
  const { entries } = roots(doc)
  const targets = Array.from(new Set(ids))
    .map((id) => entries.get(id))
    .filter((entry): entry is YEntry => entry !== undefined)
  let changed = 0
  const work = () => {
    for (const entry of targets) {
      const teacher = entryParticipant(entry) === 'teacher'
      const language = entryLanguage(entry)
      let participant: Participant = teacher ? 'teacher' : 'student'
      let nextLanguage: Language = language
      if (change === 'toStudent' && teacher) {
        participant = 'student'
        nextLanguage = 'latin'
      } else if (change === 'toTeacher' && !teacher) {
        participant = 'teacher'
        nextLanguage = 'latin'
      } else if (change === 'toL1' && teacher) nextLanguage = 'l1'
      else if (change === 'toLatin' && teacher) nextLanguage = 'latin'
      else if (change === 'toggleLanguage' && teacher) nextLanguage = language === 'l1' ? 'latin' : 'l1'
      if (participant === entryParticipant(entry) && nextLanguage === language) continue
      if (participant !== entry.get('participant')) entry.set('participant', participant)
      if (nextLanguage !== entry.get('language')) entry.set('language', nextLanguage)
      changed++
    }
  }
  // Check first, so a no-op does not create an empty undo step.
  const wouldChange = targets.some((entry) => {
    const teacher = entryParticipant(entry) === 'teacher'
    switch (change) {
      case 'toStudent':
        return teacher
      case 'toTeacher':
        return !teacher
      case 'toL1':
        return teacher && entryLanguage(entry) === 'latin'
      case 'toLatin':
        return teacher && entryLanguage(entry) === 'l1'
      case 'toggleLanguage':
        return teacher
    }
  })
  if (wouldChange) transact(doc, work)
  return changed
}

/** Whether `ids` form one run in script order. */
export function isContiguous(structure: Structure, ids: Iterable<string>): boolean {
  const positions = Array.from(new Set(ids))
    .map((id) => structure.flat.indexOf(id))
    .sort((a, b) => a - b)
  if (positions.length === 0 || positions[0] < 0) return false
  return positions[positions.length - 1] - positions[0] + 1 === positions.length
}

/** Where an entry is: lesson index and index within the lesson. */
export function locate(structure: Structure, id: string): [number, number] | null {
  for (let l = 0; l < structure.lessons.length; l++) {
    const e = structure.lessons[l].entryIds.indexOf(id)
    if (e >= 0) return [l, e]
  }
  return null
}

/** Whether a contiguous block (or one entry) can move one place up / down:
 * not from the very first / last position of the script. */
export function canMove(structure: Structure, ids: Iterable<string>, direction: -1 | 1): boolean {
  const list = Array.from(new Set(ids))
  if (!isContiguous(structure, list)) return false
  const sorted = list.sort((a, b) => structure.flat.indexOf(a) - structure.flat.indexOf(b))
  if (direction < 0) {
    const [l, e] = locate(structure, sorted[0])!
    return e > 0 || l > 0
  }
  const [l, e] = locate(structure, sorted[sorted.length - 1])!
  return e + 1 < structure.lessons[l].entryIds.length || l + 1 < structure.lessons.length
}

/**
 * Move a contiguous block of entries (one entry or more) one place up or
 * down, as the desktop did (FEATURES §6):
 *
 * - From the first position of a lesson, Move up sends the block's entries
 *   in that lesson to the end of the previous lesson; from the last
 *   position, Move down sends them to the start of the next lesson.
 * - Otherwise the block swaps with its neighbour; positions keep their
 *   lesson, so a block spanning a lesson boundary shifts across it.
 *
 * Returns whether anything moved.
 */
export function moveBlock(doc: Y.Doc, ids: Iterable<string>, direction: -1 | 1): boolean {
  const structure = readStructure(doc)
  const chosen = Array.from(new Set(ids))
  if (!isContiguous(structure, chosen)) return false
  const layout = cloneLayout(structure)
  // Slots (lesson index, entry index) in script order.
  const slots: [number, number][] = []
  layout.forEach((lesson, l) => lesson.entryIds.forEach((_, e) => slots.push([l, e])))
  const flatPositions = chosen.map((id) => structure.flat.indexOf(id)).sort((a, b) => a - b)
  const first = slots[flatPositions[0]]
  const last = slots[flatPositions[flatPositions.length - 1]]
  const prefer = new Set<string>()

  if (direction < 0) {
    if (first[1] === 0) {
      if (first[0] === 0) return false
      const lesson = layout[first[0]]
      const count = flatPositions.filter((p) => slots[p][0] === first[0]).length
      const moved = lesson.entryIds.splice(0, count)
      layout[first[0] - 1].entryIds.push(...moved)
      moved.forEach((id) => prefer.add(id))
    } else {
      const run = [[first[0], first[1] - 1] as [number, number], ...flatPositions.map((p) => slots[p])]
      rotate(layout, run, true, prefer)
    }
  } else {
    if (last[1] === layout[last[0]].entryIds.length - 1) {
      if (last[0] === layout.length - 1) return false
      const lesson = layout[last[0]]
      const count = flatPositions.filter((p) => slots[p][0] === last[0]).length
      const moved = lesson.entryIds.splice(lesson.entryIds.length - count, count)
      layout[last[0] + 1].entryIds.unshift(...moved)
      moved.forEach((id) => prefer.add(id))
    } else {
      const run = [...flatPositions.map((p) => slots[p]), [last[0], last[1] + 1] as [number, number]]
      rotate(layout, run, false, prefer)
    }
  }
  transact(doc, () => applyLayout(doc, layout, prefer))
  return true
}

/** Rotate the values in `slots` by one. `up`: the neighbour before the block
 * moves to the last slot. The neighbour is preferred for re-keying when the
 * block stays in one lesson (one key changes instead of many). */
function rotate(layout: StructureLesson[], slots: [number, number][], up: boolean, prefer: Set<string>) {
  const values = slots.map(([l, e]) => layout[l].entryIds[e])
  const neighbour = up ? values[0] : values[values.length - 1]
  const rotated = up ? [...values.slice(1), values[0]] : [values[values.length - 1], ...values.slice(0, -1)]
  slots.forEach(([l, e], i) => {
    layout[l].entryIds[e] = rotated[i]
  })
  prefer.add(neighbour)
}

// ---- project details -------------------------------------------------------

export type MetaChange = Partial<Pick<Meta, 'name' | 'l1Name' | 'gapSameMs' | 'gapDifferentMs' | 'paper' | 'fontSize' | 'spacedRepetition'>>

/** Set project details. Gaps are clamped to 0–60 s and font size to 8–30. */
export function setMeta(doc: Y.Doc, change: MetaChange): void {
  const { meta } = roots(doc)
  const values: Record<string, unknown> = { ...change }
  for (const key of ['gapSameMs', 'gapDifferentMs'] as const) {
    if (values[key] !== undefined) values[key] = Math.round(Math.min(MAX_GAP_MS, Math.max(0, Number(values[key]) || 0)))
  }
  if (values.fontSize !== undefined) values.fontSize = Math.min(30, Math.max(8, Number(values.fontSize) || 15))
  const changed = Object.entries(values).filter(([key, value]) => value !== undefined && meta.get(key) !== value)
  if (changed.length === 0) return
  transact(doc, () => {
    for (const [key, value] of changed) meta.set(key, value)
  })
}

// ---- repair ----------------------------------------------------------------

/** Remove entries whose lesson no longer exists (DESIGN.md: "removed by the
 * next client that notices"). A system change: never undone. */
export function removeOrphans(doc: Y.Doc): number {
  const { orphans } = readStructure(doc)
  if (orphans.length === 0) return 0
  const { entries } = roots(doc)
  doc.transact(() => {
    for (const id of orphans) entries.delete(id)
  }, SYSTEM_ORIGIN)
  return orphans.length
}
