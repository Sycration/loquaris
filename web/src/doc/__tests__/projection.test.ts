import { describe, expect, it, vi } from 'vitest'
import * as Y from 'yjs'
import { addEntry, deleteLesson, moveBlock, setMeta } from '../ops'
import { DocStore } from '../projection'
import { createEntry, LOCAL_ORIGIN, roots } from '../schema'
import { buildDoc, clone, connect } from './helpers'

describe('DocStore', () => {
  it('projects ordered lessons and entries', () => {
    const { doc, ids, lessonIds } = buildDoc([['t:a', 's:b'], ['T:c']])
    const store = new DocStore(doc)
    const structure = store.getStructure()
    expect(structure.lessons.map((l) => l.id)).toEqual(lessonIds)
    expect(structure.flat).toEqual([ids.a, ids.b, ids.c])
    expect(structure.position.get(ids.c)).toEqual({ lesson: 1, index: 0, flat: 2 })
    expect(store.getEntry(ids.b)).toMatchObject({ participant: 'student', language: 'latin', text: 'b' })
  })

  it('notifies only the edited entry when text changes', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b']])
    const store = new DocStore(doc)
    const onA = vi.fn()
    const onB = vi.fn()
    const onStructure = vi.fn()
    const onRecordings = vi.fn()
    store.subscribeEntry(ids.a, onA)
    store.subscribeEntry(ids.b, onB)
    store.subscribeStructure(onStructure)
    store.subscribeRecordings(onRecordings)
    const before = store.getStructure()
    doc.transact(() => (roots(doc).entries.get(ids.a)!.get('text') as Y.Text).insert(1, 'x'), LOCAL_ORIGIN)
    expect(onA).toHaveBeenCalledTimes(1)
    expect(onB).not.toHaveBeenCalled()
    expect(onStructure).not.toHaveBeenCalled()
    expect(onRecordings).not.toHaveBeenCalled()
    expect(store.getEntry(ids.a)!.text).toBe('ax')
    expect(store.getStructure()).toBe(before)
  })

  it('keeps unchanged lesson objects across structural changes', () => {
    const { doc, ids } = buildDoc([['t:a', 't:b'], ['t:c']])
    const store = new DocStore(doc)
    const before = store.getStructure()
    moveBlock(doc, [ids.b], -1)
    const after = store.getStructure()
    expect(after).not.toBe(before)
    expect(after.lessons[0]).not.toBe(before.lessons[0])
    expect(after.lessons[1]).toBe(before.lessons[1])
  })

  it('updates recording summaries when clips or gaps change', () => {
    const { doc, ids, lessonIds } = buildDoc([['t:a', 's:b']])
    const store = new DocStore(doc)
    expect(store.lessonSummary(lessonIds[0])).toBe('Recordings incomplete')
    doc.transact(() => {
      roots(doc).entries.get(ids.a)!.set('audio', { blob: 'x', sampleRate: 10, frames: 10, channels: 1 })
      roots(doc).entries.get(ids.b)!.set('audio', { blob: 'y', sampleRate: 10, frames: 20, channels: 1 })
    })
    expect(store.lessonSummary(lessonIds[0])).toBe('00:03.5')
    setMeta(doc, { gapDifferentMs: 1000 })
    expect(store.lessonSummary(lessonIds[0])).toBe('00:04.0')
  })

  it('reports orphans for cleanup', async () => {
    const { doc } = buildDoc([['t:a'], ['t:b']])
    const store = new DocStore(doc)
    const onOrphans = vi.fn()
    store.onOrphans = onOrphans
    roots(doc).entries.set('orphan0000000000', createEntry({ lesson: 'missing', order: 'a0', participant: 'teacher', language: 'l1' }))
    await Promise.resolve()
    expect(onOrphans).toHaveBeenCalledWith(['orphan0000000000'])
  })

  it('follows remote changes', () => {
    const { doc: a, lessonIds } = buildDoc([['t:a'], []])
    const b = clone(a)
    connect(a, b)
    const store = new DocStore(b)
    const onStructure = vi.fn()
    store.subscribeStructure(onStructure)
    addEntry(a, { lesson: lessonIds[0], after: null, participant: 'student', language: 'latin', text: 'remote' })
    expect(onStructure).toHaveBeenCalledTimes(1)
    expect(store.getStructure().flat.length).toBe(2)
    expect(deleteLesson(a, lessonIds[0])).toBe(true)
    expect(store.getStructure().lessons.map((l) => l.id)).toEqual([lessonIds[1]])
    expect(store.getStructure().flat).toEqual([])
  })
})
