import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import * as Y from 'yjs'
import { buildDoc } from '../../doc/__tests__/helpers'
import { addEntry, moveBlock, deleteEntries, setMeta } from '../../doc/ops'
import { DocStore } from '../../doc/projection'
import { createUndoManager } from '../../doc/undo'
import { entryLemmaChoices, LOCAL_ORIGIN, roots } from '../../doc/schema'
import type { Answer, EntryContent, Gaps, ProjectContent, ReviewFlag, SearchResult } from '../client'
import { writeReview } from '../review'
import { AnalysisSession, entryContent, type AnalysisApi } from '../session'

class FakeClient implements AnalysisApi {
  calls: { name: string; arg?: unknown }[] = []
  revision = 0
  onRestart?: () => void
  searchResults: Promise<Answer<SearchResult>>[] = []
  private resolveStart!: () => void
  private ready = new Promise<void>((r) => (this.resolveStart = r))
  becomeReady() {
    this.resolveStart()
  }
  start = () => this.ready
  dispose = vi.fn()
  private change(name: string, arg?: unknown) {
    this.calls.push({ name, arg })
    this.revision++
    return Promise.resolve()
  }
  setProject = (p: ProjectContent) => this.change('setProject', p)
  setStructure = (l: unknown) => this.change('setStructure', l)
  setEntry = (e: EntryContent) => this.change('setEntry', e)
  removeEntry = (id: string) => this.change('removeEntry', id)
  setL1Name = (n: string) => this.change('setL1Name', n)
  setGaps = (g: Gaps) => this.change('setGaps', g)
  spacedRepetition = vi.fn(() => Promise.resolve<Answer<ReviewFlag[]>>({ revision: this.revision, value: [] }))
  search = vi.fn((query: string) => {
    void query
    return this.searchResults.shift() ?? Promise.resolve({ revision: this.revision, value: emptyResult() })
  })
  firstUse = vi.fn(() => Promise.resolve({ revision: this.revision, value: {} }))
  newInLesson = vi.fn()
  review = vi.fn()
  applyReview = vi.fn()
  isCurrent = (a: Answer<unknown>) => a.revision === this.revision
  names() {
    return this.calls.map((c) => c.name)
  }
}

function emptyResult(): SearchResult {
  return { mode: 'text', lemma: null, hits: [], perLesson: [], lessonsWithHits: 0, analysis: [] }
}

function setup(lessons: string[][] = [['T:amo', 't:I love'], ['T:amas']]) {
  const built = buildDoc(lessons)
  const store = new DocStore(built.doc)
  const client = new FakeClient()
  let options: { onRestart?: () => void } = {}
  const session = new AnalysisSession(store, {
    debounceMs: 100,
    createClient: (o) => {
      options = o
      return client
    },
  })
  return { ...built, store, client, session, restart: () => options.onRestart?.() }
}

const text = (doc: Y.Doc, id: string) => roots(doc).entries.get(id)!.get('text') as Y.Text

beforeEach(() => vi.useFakeTimers())
afterEach(() => vi.useRealTimers())

describe('feeding the worker', () => {
  it('sends the project first, then ready', async () => {
    const { session, client, ids } = setup()
    const started = session.start()
    expect(session.status).toBe('loading')
    expect(client.names()).toEqual(['setProject', 'setGaps'])
    const project = client.calls[0].arg as ProjectContent
    expect(project.lessons.map((l) => l.entries.map((e) => e.id))).toEqual([[ids.amo, ids['I love']], [ids.amas]])
    expect(project.lessons[0].entries[0]).toMatchObject({ participant: 'teacher', language: 'latin', text: 'amo', lemmas: [] })
    client.becomeReady()
    await started
    expect(session.status).toBe('ready')
  })

  it('debounces text edits per entry', () => {
    const { session, client, doc, ids } = setup()
    void session.start()
    client.calls.length = 0
    doc.transact(() => text(doc, ids.amo).insert(3, 'x'), LOCAL_ORIGIN)
    doc.transact(() => text(doc, ids.amo).insert(4, 'y'), LOCAL_ORIGIN)
    vi.advanceTimersByTime(99)
    expect(client.calls).toEqual([])
    vi.advanceTimersByTime(2)
    expect(client.calls).toHaveLength(1)
    expect(client.calls[0]).toMatchObject({ name: 'setEntry', arg: { id: ids.amo, text: 'amoxy' } })
  })

  it('skips entry changes the worker does not care about', () => {
    const { session, client, doc, ids } = setup()
    void session.start()
    client.calls.length = 0
    doc.transact(() => roots(doc).entries.get(ids.amo)!.set('trim', { start: 0, end: 1 }), LOCAL_ORIGIN)
    vi.advanceTimersByTime(200)
    expect(client.calls).toEqual([])
  })

  it('sends structure after moves, inserts and deletes; removeEntry after delete', () => {
    const { session, client, doc, ids, lessonIds, store } = setup()
    void session.start()
    client.calls.length = 0
    moveBlock(doc, [ids.amo], 1)
    expect(client.names()).toEqual(['setStructure'])
    client.calls.length = 0
    const id = addEntry(doc, { lesson: lessonIds[1], after: null, participant: 'teacher', language: 'latin', text: 'new' })!
    expect(client.names()).toEqual(['setEntry', 'setStructure'])
    expect(client.calls[0].arg).toMatchObject({ id, text: 'new' })
    expect(client.calls[1].arg).toEqual(store.getStructure().lessons.map((l) => ({ id: l.id, entryIds: l.entryIds })))
    client.calls.length = 0
    deleteEntries(doc, [ids.amas])
    expect(client.names()).toEqual(['removeEntry', 'setStructure'])
    // Edits to a deleted entry are not sent.
    vi.advanceTimersByTime(200)
    expect(client.names()).toEqual(['removeEntry', 'setStructure'])
  })

  it('sends setL1Name when the L1 changes', () => {
    const { session, client, doc } = setup()
    void session.start()
    client.calls.length = 0
    setMeta(doc, { l1Name: 'Français' })
    expect(client.calls).toEqual([{ name: 'setL1Name', arg: 'Français' }])
  })

  it('sends each entry\'s cropped clip duration, and none without a recording', () => {
    const { session, client, doc, ids } = setup()
    const entries = roots(doc).entries
    entries.get(ids.amo)!.set('audio', { blob: 'x', sampleRate: 100, frames: 500, channels: 1 })
    entries.get(ids.amas)!.set('audio', { blob: 'y', sampleRate: 100, frames: 500, channels: 1 })
    entries.get(ids.amas)!.set('trim', { start: 100, end: 300 })
    void session.start()
    const project = client.calls[0].arg as ProjectContent
    const byId = Object.fromEntries(project.lessons.flatMap((l) => l.entries).map((e) => [e.id, e]))
    expect(byId[ids.amo].duration).toBe(5)
    expect(byId[ids.amas].duration).toBe(2)
    expect('duration' in byId[ids['I love']]).toBe(false)
  })

  it('sends a changed duration when a recording or its trim changes, and removing it omits it', () => {
    const { session, client, doc, ids } = setup()
    void session.start()
    client.calls.length = 0
    const entry = roots(doc).entries.get(ids.amo)!
    entry.set('audio', { blob: 'x', sampleRate: 100, frames: 500, channels: 1 })
    vi.advanceTimersByTime(200)
    expect(client.calls).toMatchObject([{ name: 'setEntry', arg: { id: ids.amo, duration: 5 } }])
    entry.set('trim', { start: 0, end: 150 })
    vi.advanceTimersByTime(200)
    expect(client.calls[1]).toMatchObject({ name: 'setEntry', arg: { duration: 1.5 } })
    entry.delete('audio')
    vi.advanceTimersByTime(200)
    expect('duration' in (client.calls[2].arg as EntryContent)).toBe(false)
  })

  it('sends the gaps after the project, and again when they change', () => {
    const { session, client, doc } = setup()
    void session.start()
    expect(client.calls.slice(0, 2)).toEqual([
      { name: 'setProject', arg: expect.anything() },
      { name: 'setGaps', arg: { sameParticipantMs: 100, differentParticipantMs: 500 } },
    ])
    client.calls.length = 0
    setMeta(doc, { gapDifferentMs: 1500 })
    expect(client.calls).toEqual([{ name: 'setGaps', arg: { sameParticipantMs: 100, differentParticipantMs: 1500 } }])
    setMeta(doc, { name: 'x' })
    expect(client.calls).toHaveLength(1)
  })

  it('queries spaced repetition flags (debounced) only while the project enables them', async () => {
    const { session, client, doc } = setup()
    const start = session.start()
    client.becomeReady()
    await start
    const unsubscribe = session.spaced.subscribe(() => {})
    vi.advanceTimersByTime(1000)
    expect(client.spacedRepetition).toHaveBeenCalledTimes(1)
    // A burst of edits is one query.
    for (let i = 0; i < 3; i++) setMeta(doc, { gapSameMs: 200 + i })
    vi.advanceTimersByTime(1000)
    expect(client.spacedRepetition).toHaveBeenCalledTimes(2)
    // Switched off: no more queries, and the flags are empty.
    setMeta(doc, { spacedRepetition: false })
    vi.advanceTimersByTime(1000)
    await Promise.resolve()
    expect(client.spacedRepetition).toHaveBeenCalledTimes(2)
    expect(session.spaced.getSnapshot()).toEqual([])
    setMeta(doc, { spacedRepetition: true })
    vi.advanceTimersByTime(1000)
    expect(client.spacedRepetition).toHaveBeenCalledTimes(3)
    unsubscribe()
  })

  it('resends the project after a restart', () => {
    const { session, client, restart } = setup()
    void session.start()
    client.calls.length = 0
    restart()
    expect(client.names()).toEqual(['setProject', 'setGaps'])
  })

  it('converts lemma choices', () => {
    const { store, ids, doc } = setup()
    doc.transact(() => roots(doc).entries.get(ids.amo)!.set('lemmas', new Y.Map(Object.entries({ 'amo#1': { key: 'amo', extension: true } }))), LOCAL_ORIGIN)
    expect(entryContent(store.getEntry(ids.amo)!).lemmas).toEqual([{ token: 'amo', occurrence: 1, lemmaKey: 'amo', extension: true }])
  })
})

describe('search answers', () => {
  async function ready() {
    const s = setup()
    const started = s.session.start()
    s.client.becomeReady()
    await started
    return s
  }
  const hit = (entryId: string) => ({ ...emptyResult(), hits: [{ entryId, start: 0, end: 2 }] })

  it('drops answers for an older revision', async () => {
    const { session, client, doc, ids } = await ready()
    session.search.subscribe(() => {})
    client.searchResults.push(Promise.resolve({ revision: client.revision, value: hit(ids.amo) }))
    // The answer is computed, then the doc changes before it is delivered.
    const stale = Promise.resolve({ revision: client.revision, value: hit(ids.amo) })
    client.searchResults[0] = stale.then((a) => {
      client.revision++
      return a
    })
    session.search.setQuery('am')
    await vi.advanceTimersByTimeAsync(0)
    expect(session.search.getResult()).toBeNull()
    void doc
  })

  it('keeps only the latest search when answers arrive out of order', async () => {
    const { session, client, ids } = await ready()
    session.search.subscribe(() => {})
    let resolveFirst!: (a: Answer<SearchResult>) => void
    client.searchResults.push(new Promise((r) => (resolveFirst = r)))
    session.search.setQuery('a')
    client.searchResults.push(Promise.resolve({ revision: client.revision, value: hit(ids.amas) }))
    session.search.setQuery('am')
    await vi.advanceTimersByTimeAsync(0)
    resolveFirst({ revision: client.revision, value: hit(ids.amo) })
    await vi.advanceTimersByTimeAsync(0)
    expect(session.search.rangesFor(ids.amas)).toEqual([{ start: 0, end: 2 }])
    expect(session.search.rangesFor(ids.amo)).toEqual([])
  })

  it('picking a lemma sets query, turns exactText off; editing drops the lemma', async () => {
    const { session, client } = await ready()
    session.search.subscribe(() => {})
    session.search.setExactText(true)
    session.search.pickLemma({ key: 'amo', extension: false, headword: 'amō' })
    expect(session.search.getState()).toMatchObject({ query: 'amō', exactText: false, lemma: { key: 'amo', extension: false } })
    await vi.advanceTimersByTimeAsync(0)
    expect(client.search).toHaveBeenLastCalledWith('amō', { exactText: false, unmarkedShort: false, lemma: { key: 'amo', extension: false } })
    session.search.setQuery('amō ')
    expect(session.search.getState().lemma).toBeNull()
  })

  it('re-runs the search when content changes', async () => {
    const { session, client, doc, ids } = await ready()
    session.search.subscribe(() => {})
    session.search.setQuery('am')
    await vi.advanceTimersByTimeAsync(0)
    const before = client.search.mock.calls.length
    doc.transact(() => text(doc, ids.amo).insert(0, 'x'), LOCAL_ORIGIN)
    await vi.advanceTimersByTimeAsync(150)
    expect(client.search.mock.calls.length).toBeGreaterThan(before)
  })
})

describe('writeReview', () => {
  it('writes text and lemma choices as one undo step', () => {
    const { doc, ids } = buildDoc([['T:amo, amas']])
    const undo = createUndoManager(doc)
    doc.transact(() => roots(doc).entries.get(ids['amo, amas'])!.set('lemmas', new Y.Map(Object.entries({ 'old#0': { key: 'old', extension: false } }))), LOCAL_ORIGIN)
    undo.stopCapturing()
    const entry = ids['amo, amas']
    const ok = writeReview(doc, undo, entry, {
      text: 'amō, amās',
      changed: true,
      lemmas: [{ token: 'amo', occurrence: 0, lemmaKey: 'amo', extension: false }],
    })
    expect(ok).toBe(true)
    expect(text(doc, entry).toString()).toBe('amō, amās')
    expect(entryLemmaChoices(roots(doc).entries.get(entry)!)).toEqual({ 'amo#0': { key: 'amo', extension: false } })
    undo.undo()
    expect(text(doc, entry).toString()).toBe('amo, amas')
    expect(entryLemmaChoices(roots(doc).entries.get(entry)!)).toEqual({ 'old#0': { key: 'old', extension: false } })
  })

  it('clears choices with an empty list and ignores missing entries', () => {
    const { doc, ids } = buildDoc([['T:amo']])
    doc.transact(() => roots(doc).entries.get(ids.amo)!.set('lemmas', new Y.Map(Object.entries({ 'amo#0': { key: 'amo', extension: false } }))), LOCAL_ORIGIN)
    expect(writeReview(doc, null, ids.amo, { text: 'amo', lemmas: [], changed: true })).toBe(true)
    expect(entryLemmaChoices(roots(doc).entries.get(ids.amo)!)).toEqual({})
    expect(writeReview(doc, null, 'nope', { text: 'x', lemmas: [], changed: true })).toBe(false)
  })
})
