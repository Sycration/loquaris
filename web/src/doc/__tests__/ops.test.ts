import { describe, expect, it } from 'vitest'
import * as Y from 'yjs'
import {
  addEntry,
  addLesson,
  canMove,
  changeKind,
  deleteEntries,
  deleteLesson,
  duplicateEntries,
  duplicateLesson,
  isContiguous,
  moveBlock,
  moveLesson,
  readStructure,
  removeOrphans,
  setMeta,
} from '../ops'
import { ID_LENGTH } from '../ids'
import {
  createEntry,
  entryLemmaChoices,
  entryTrim,
  lessonNameString,
  readMeta,
  roots,
  SCHEMA_VERSION,
  LOCAL_ORIGIN,
} from '../schema'
import { createUndoManager, asUndoStep } from '../undo'
import { buildDoc, kinds, texts } from './helpers'

describe('ids and creation', () => {
  it('creates 16-character base-62 ids', () => {
    const { ids, lessonIds } = buildDoc([['t:a']])
    for (const id of [...Object.values(ids), ...lessonIds]) expect(id).toMatch(new RegExp(`^[0-9A-Za-z]{${ID_LENGTH}}$`))
  })

  it('adds entries after the given entry or at the end', () => {
    const { doc, ids, lessonIds } = buildDoc([['t:a', 't:c']])
    addEntry(doc, { lesson: lessonIds[0], after: ids.a, participant: 'teacher', language: 'l1', text: 'b' })
    addEntry(doc, { lesson: lessonIds[0], after: null, participant: 'teacher', language: 'l1', text: 'd' })
    expect(texts(doc)).toEqual([['a', 'b', 'c', 'd']])
  })

  it('forces students to Latin', () => {
    const { doc, lessonIds } = buildDoc([[]])
    addEntry(doc, { lesson: lessonIds[0], after: null, participant: 'student', language: 'l1', text: 'x' })
    expect(kinds(doc)).toEqual([['s:x']])
    expect(roots(doc).entries.values().next().value!.get('language')).toBe('latin')
  })

  it('reads defaults for missing meta fields and accepts bigint numbers', () => {
    const doc = new Y.Doc()
    const { meta } = roots(doc)
    expect(readMeta(meta).l1Name).toBe('English')
    meta.set('gapSameMs', BigInt(250) as unknown as number)
    meta.set('schema', SCHEMA_VERSION)
    expect(readMeta(meta).gapSameMs).toBe(250)
    expect(readMeta(meta).gapDifferentMs).toBe(500)
  })

  it('spaced repetition is on unless switched off, and the switch is undoable', () => {
    const doc = new Y.Doc()
    const undo = createUndoManager(doc)
    expect(readMeta(roots(doc).meta).spacedRepetition).toBe(true)
    setMeta(doc, { spacedRepetition: false })
    expect(readMeta(roots(doc).meta).spacedRepetition).toBe(false)
    undo.undo()
    expect(readMeta(roots(doc).meta).spacedRepetition).toBe(true)
  })
})

describe('participant and language', () => {
  it('follows the student-speaks-Latin rules', () => {
    const { doc, ids } = buildDoc([['t:a', 'T:b', 's:c']])
    changeKind(doc, [ids.a], 'toStudent')
    expect(kinds(doc)).toEqual([['s:a', 'T:b', 's:c']])
    changeKind(doc, [ids.c], 'toTeacher')
    expect(kinds(doc)).toEqual([['s:a', 'T:b', 'T:c']])
    // Only teachers switch language.
    changeKind(doc, [ids.a, ids.b], 'toggleLanguage')
    expect(kinds(doc)).toEqual([['s:a', 't:b', 'T:c']])
    changeKind(doc, [ids.b, ids.c], 'toLatin')
    expect(kinds(doc)).toEqual([['s:a', 'T:b', 'T:c']])
  })

  it('does not create an undo step for a no-op', () => {
    const { doc, ids } = buildDoc([['s:a']])
    const undo = createUndoManager(doc)
    expect(changeKind(doc, [ids.a], 'toStudent')).toBe(0)
    expect(undo.undoStack.length).toBe(0)
  })
})

describe('moves', () => {
  it('moves one entry within a lesson and across lesson boundaries', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b'], ['t:c', 't:d']])
    expect(moveBlock(doc, [ids.b], -1)).toBe(true)
    expect(texts(doc)).toEqual([['b', 'a'], ['c', 'd']])
    // Last of a lesson goes to the start of the next.
    moveBlock(doc, [ids.a], 1)
    expect(texts(doc)).toEqual([['b'], ['a', 'c', 'd']])
    // First of a lesson goes to the end of the previous.
    moveBlock(doc, [ids.a], -1)
    expect(texts(doc)).toEqual([['b', 'a'], ['c', 'd']])
    // Not from the very first / last position.
    expect(moveBlock(doc, [ids.b], -1)).toBe(false)
    expect(moveBlock(doc, [ids.d], 1)).toBe(false)
  })

  it('moves into an empty neighbouring lesson', () => {
    const { doc, ids } = buildDoc([[], ['t:a']])
    expect(canMove(readStructure(doc), [ids.a], -1)).toBe(true)
    moveBlock(doc, [ids.a], -1)
    expect(texts(doc)).toEqual([['a'], []])
  })

  it('moves a contiguous block, keeping its order', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b', 't:c', 't:d']])
    moveBlock(doc, [ids.b, ids.c], -1)
    expect(texts(doc)).toEqual([['b', 'c', 'a', 'd']])
    moveBlock(doc, [ids.b, ids.c], 1)
    moveBlock(doc, [ids.b, ids.c], 1)
    expect(texts(doc)).toEqual([['a', 'd', 'b', 'c']])
  })

  it('changes one key when a block swaps with its neighbour', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b', 't:c', 't:d']])
    const before = new Map(Array.from(roots(doc).entries.entries(), ([id, e]) => [id, e.get('order')]))
    moveBlock(doc, [ids.b, ids.c, ids.d], -1)
    const changed = Array.from(roots(doc).entries.entries()).filter(([id, e]) => e.get('order') !== before.get(id))
    expect(changed.map(([id]) => id)).toEqual([ids.a])
  })

  it('moves a block across lesson boundaries like the desktop', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b'], ['t:c', 't:d']])
    // Block spanning the boundary, not at a lesson start: slots keep lessons.
    moveBlock(doc, [ids.b, ids.c], -1)
    expect(texts(doc)).toEqual([['b', 'c'], ['a', 'd']])
    // Block at the start of lesson 1: its part in lesson 1 goes to lesson 0's end.
    const second = buildDoc([['t:a'], ['t:b', 't:c', 't:d']])
    moveBlock(second.doc, [second.ids.b, second.ids.c], -1)
    expect(texts(second.doc)).toEqual([['a', 'b', 'c'], ['d']])
    // Block at the end of lesson 0 moving down: to the start of lesson 1.
    const third = buildDoc([['t:a', 't:b', 't:c'], ['t:d']])
    moveBlock(third.doc, [third.ids.b, third.ids.c], 1)
    expect(texts(third.doc)).toEqual([['a'], ['b', 'c', 'd']])
  })

  it('refuses non-contiguous blocks', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b', 't:c']])
    expect(isContiguous(readStructure(doc), [ids.a, ids.c])).toBe(false)
    expect(moveBlock(doc, [ids.a, ids.c], 1)).toBe(false)
    expect(isContiguous(readStructure(doc), [ids.c, ids.b])).toBe(true)
  })
})

describe('duplicate and delete', () => {
  it('duplicates after each target with audio, crop and lemma choices', () => {
    const { doc, ids } = buildDoc([['t:a', 'T:b'], ['s:c']])
    const entry = roots(doc).entries.get(ids.b)!
    doc.transact(() => {
      entry.set('audio', { blob: 'ab', sampleRate: 48000, frames: 480, channels: 1 })
      entry.set('trim', { start: 10, end: 20 })
      ;(entry.get('lemmas') as Y.Map<unknown>).set('puella#0', { key: 'puella', extension: false })
    })
    const copies = duplicateEntries(doc, [ids.b, ids.c, ids.a])
    expect(texts(doc)).toEqual([['a', 'a', 'b', 'b'], ['c', 'c']])
    const copy = roots(doc).entries.get(copies.get(ids.b)!)!
    expect(copy.get('audio')).toEqual({ blob: 'ab', sampleRate: 48000, frames: 480, channels: 1 })
    expect(entryTrim(copy)).toEqual({ start: 10, end: 20 })
    expect(entryLemmaChoices(copy)).toEqual({ 'puella#0': { key: 'puella', extension: false } })
    // The copy's text is independent.
    ;(copy.get('text') as Y.Text).insert(1, '!')
    expect(texts(doc)).toEqual([['a', 'a', 'b', 'b!'], ['c', 'c']])
  })

  it('deletes entries and lessons with their entries', () => {
    const { doc, ids, lessonIds } = buildDoc([['t:a', 't:b'], ['t:c']])
    deleteEntries(doc, [ids.a])
    expect(texts(doc)).toEqual([['b'], ['c']])
    expect(deleteLesson(doc, lessonIds[1])).toBe(true)
    expect(texts(doc)).toEqual([['b']])
    expect(roots(doc).entries.has(ids.c)).toBe(false)
    // The last lesson stays.
    expect(deleteLesson(doc, lessonIds[0])).toBe(false)
  })
})

describe('lessons', () => {
  it('adds, reorders and duplicates lessons', () => {
    const { doc, lessonIds } = buildDoc([['t:a'], ['t:b']])
    const middle = addLesson(doc, lessonIds[0], 'middle')
    expect(readStructure(doc).lessons.map((l) => l.id)).toEqual([lessonIds[0], middle, lessonIds[1]])
    moveLesson(doc, lessonIds[1], -1)
    moveLesson(doc, lessonIds[1], -1)
    expect(texts(doc)).toEqual([['b'], ['a'], []])
    expect(moveLesson(doc, lessonIds[1], -1)).toBe(false)
    const copy = duplicateLesson(doc, lessonIds[0])!
    expect(texts(doc)).toEqual([['b'], ['a'], ['a'], []])
    expect(lessonNameString(roots(doc).lessons.get(copy)!)).toBe('')
    expect(lessonNameString(roots(doc).lessons.get(middle)!)).toBe('middle')
  })

  it('undoes a lesson deletion with its entries', () => {
    const { doc, lessonIds } = buildDoc([['t:a'], ['t:b', 't:c']])
    const undo = createUndoManager(doc)
    asUndoStep(undo, () => deleteLesson(doc, lessonIds[1]))
    expect(texts(doc)).toEqual([['a']])
    undo.undo()
    expect(texts(doc)).toEqual([['a'], ['b', 'c']])
  })
})

describe('meta', () => {
  it('clamps gaps to 0–60 s', () => {
    const { doc } = buildDoc([[]])
    setMeta(doc, { gapSameMs: -5, gapDifferentMs: 120_000, name: 'Lectiones' })
    expect(readMeta(roots(doc).meta)).toMatchObject({ gapSameMs: 0, gapDifferentMs: 60_000, name: 'Lectiones' })
  })
})

describe('undo', () => {
  it('tracks only local changes and groups typing', () => {
    const { doc, ids } = buildDoc([['t:a']])
    const undo = createUndoManager(doc)
    const text = roots(doc).entries.get(ids.a)!.get('text') as Y.Text
    doc.transact(() => text.insert(1, 'b'), LOCAL_ORIGIN)
    doc.transact(() => text.insert(2, 'c'), LOCAL_ORIGIN)
    doc.transact(() => text.insert(3, 'Z'), 'remote')
    undo.undo()
    expect(text.toString()).toBe('aZ')
  })

  it('makes each structural change its own step', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b']])
    const undo = createUndoManager(doc)
    const text = roots(doc).entries.get(ids.a)!.get('text') as Y.Text
    doc.transact(() => text.insert(1, '1'), LOCAL_ORIGIN)
    asUndoStep(undo, () => moveBlock(doc, [ids.b], -1))
    expect(texts(doc)).toEqual([['b', 'a1']])
    undo.undo()
    expect(texts(doc)).toEqual([['a1', 'b']])
    undo.undo()
    expect(texts(doc)).toEqual([['a', 'b']])
  })
})

describe('orphans', () => {
  it('hides entries of missing lessons and removes them', () => {
    const { doc } = buildDoc([['t:a']])
    roots(doc).entries.set('orphan0000000000', createEntry({ lesson: 'gone', order: 'a0', participant: 'teacher', language: 'l1' }))
    expect(readStructure(doc).orphans).toEqual(['orphan0000000000'])
    expect(readStructure(doc).flat.length).toBe(1)
    expect(removeOrphans(doc)).toBe(1)
    expect(roots(doc).entries.size).toBe(1)
  })

  it('preserves unknown fields when editing', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b']])
    roots(doc).entries.get(ids.a)!.set('futureField', 42)
    moveBlock(doc, [ids.a], 1)
    changeKind(doc, [ids.a], 'toStudent')
    expect(roots(doc).entries.get(ids.a)!.get('futureField')).toBe(42)
  })
})
