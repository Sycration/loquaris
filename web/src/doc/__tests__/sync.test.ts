// Two replicas editing concurrently, synced afterwards: the CRDT guarantees
// convergence; these tests check that our schema gives the right *meaning*
// (no duplicates, no lost text) under concurrency.
import { describe, expect, it } from 'vitest'
import * as Y from 'yjs'
import { addEntry, deleteLesson, duplicateEntries, moveBlock, readStructure, removeOrphans } from '../ops'
import { LOCAL_ORIGIN, roots } from '../schema'
import { createUndoManager } from '../undo'
import { buildDoc, clone, connect, texts } from './helpers'

function textOf(doc: Y.Doc, id: string): Y.Text {
  return roots(doc).entries.get(id)!.get('text') as Y.Text
}

function setup(lessons: string[][]) {
  const { doc: a, ids, lessonIds } = buildDoc(lessons)
  const b = clone(a)
  const link = connect(a, b)
  return { a, b, ids, lessonIds, link }
}

describe('concurrent editing', () => {
  it('concurrent moves of the same entry do not duplicate it', () => {
    const { a, b, ids, link } = setup([['t:a', 't:b', 't:c'], ['t:d']])
    link.pause()
    moveBlock(a, [ids.b], -1)
    moveBlock(b, [ids.b], 1)
    moveBlock(b, [ids.b], 1)
    link.resume()
    expect(texts(a)).toEqual(texts(b))
    const all = texts(a).flat()
    expect(all.sort()).toEqual(['a', 'b', 'c', 'd'])
  })

  it('concurrent moves of different entries both apply', () => {
    const { a, b, ids, link } = setup([['t:a', 't:b', 't:c', 't:d', 't:e']])
    link.pause()
    moveBlock(a, [ids.b], -1) // b a c d e
    moveBlock(b, [ids.e], -1) // a b c e d
    link.resume()
    expect(texts(a)).toEqual(texts(b))
    expect(texts(a)).toEqual([['b', 'a', 'c', 'e', 'd']])
  })

  it('concurrent text edits in one entry merge character by character', () => {
    const { a, b, ids, link } = setup([['T:puella']])
    link.pause()
    a.transact(() => textOf(a, ids.puella).insert(0, 'Bona '), LOCAL_ORIGIN)
    b.transact(() => textOf(b, ids.puella).insert(6, ' cantat'), LOCAL_ORIGIN)
    link.resume()
    expect(textOf(a, ids.puella).toString()).toBe('Bona puella cantat')
    expect(textOf(b, ids.puella).toString()).toBe('Bona puella cantat')
  })

  it('a move concurrent with an edit keeps the text', () => {
    const { a, b, ids, link } = setup([['t:one', 't:two'], ['t:three']])
    link.pause()
    moveBlock(a, [ids.two], 1) // into lesson 2
    b.transact(() => textOf(b, ids.two).insert(3, ' (edited)'), LOCAL_ORIGIN)
    link.resume()
    expect(texts(a)).toEqual([['one'], ['two (edited)', 'three']])
    expect(texts(b)).toEqual(texts(a))
  })

  it('concurrent inserts at the same place both survive in a stable order', () => {
    const { a, b, ids, lessonIds, link } = setup([['t:x', 't:y']])
    link.pause()
    addEntry(a, { lesson: lessonIds[0], after: ids.x, participant: 'teacher', language: 'l1', text: 'fromA' })
    addEntry(b, { lesson: lessonIds[0], after: ids.x, participant: 'teacher', language: 'l1', text: 'fromB' })
    link.resume()
    expect(texts(a)).toEqual(texts(b))
    expect(texts(a)[0][0]).toBe('x')
    expect(texts(a)[0][3]).toBe('y')
    // Both got the same key; inserting between the tied pair still works.
    const [first] = readStructure(a).lessons[0].entryIds.slice(1, 2)
    addEntry(a, { lesson: lessonIds[0], after: first, participant: 'teacher', language: 'l1', text: 'between' })
    expect(texts(a)[0][2]).toBe('between')
    expect(texts(b)).toEqual(texts(a))
  })

  it('concurrent duplicates and moves converge', () => {
    const { a, b, ids, link } = setup([['t:a', 't:b', 't:c']])
    link.pause()
    duplicateEntries(a, [ids.b])
    moveBlock(b, [ids.c], -1)
    link.resume()
    expect(texts(a)).toEqual(texts(b))
    expect(texts(a)[0].slice().sort()).toEqual(['a', 'b', 'b', 'c'])
  })

  it('an entry added to a concurrently deleted lesson is hidden and cleaned up', () => {
    const { a, b, ids, lessonIds, link } = setup([['t:a'], ['t:b']])
    link.pause()
    deleteLesson(a, lessonIds[1])
    addEntry(b, { lesson: lessonIds[1], after: ids.b, participant: 'teacher', language: 'l1', text: 'late' })
    link.resume()
    expect(texts(a)).toEqual([['a']])
    expect(texts(b)).toEqual([['a']])
    expect(readStructure(b).orphans.length).toBe(1)
    removeOrphans(b)
    expect(roots(a).entries.size).toBe(1)
  })

  it('undo reverts only my own edits', () => {
    const { a, b, ids, link } = setup([['t:hello']])
    const undoA = createUndoManager(a)
    a.transact(() => textOf(a, ids.hello).insert(5, ' world'), LOCAL_ORIGIN)
    b.transact(() => textOf(b, ids.hello).insert(0, '>> '), LOCAL_ORIGIN)
    expect(textOf(a, ids.hello).toString()).toBe('>> hello world')
    undoA.undo()
    expect(textOf(a, ids.hello).toString()).toBe('>> hello')
    expect(textOf(b, ids.hello).toString()).toBe('>> hello')
    link.pause()
  })
})
