// Smoke test of the built wasm package in Node:
//   scripts/build-wasm.sh && node crates/core-wasm/tests/smoke.mjs
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const pkg = new URL('../../../web/src/wasm/pkg/', import.meta.url)
const { initSync, Analyzer } = await import(new URL('loquaris_core_wasm.js', pkg))
initSync({ module: readFileSync(new URL('loquaris_core_wasm_bg.wasm', pkg)) })

const analyzer = new Analyzer()
const entry = (id, participant, language, text, lemmas = [], duration) => ({ id, participant, language, text, lemmas, ...(duration === undefined ? {} : { duration }) })
analyzer.setProject(
  JSON.stringify({
    l1Name: 'English',
    lessons: [
      { id: 'a', entries: [entry('e1', 'teacher', 'l1', '🙂 The girl'), entry('e2', 'student', 'latin', 'Puellā puellam videt.')] },
      { id: 'b', entries: [entry('e3', 'teacher', 'latin', 'Mālum puellae dat')] },
    ],
  }),
)

let result = JSON.parse(analyzer.search('puellae', '{}'))
assert.equal(result.mode, 'lemma')
assert.equal(result.lemma.key, 'puella')
assert.deepEqual(result.hits, [
  { entryId: 'e2', start: 0, end: 6 },
  { entryId: 'e2', start: 7, end: 14 },
  { entryId: 'e3', start: 6, end: 13 },
])
assert.deepEqual(result.perLesson, [
  { lessonId: 'a', teacher: 0, student: 2 },
  { lessonId: 'b', teacher: 1, student: 0 },
])

// UTF-16 offsets: the emoji is two code units.
result = JSON.parse(analyzer.search('girl', JSON.stringify({ exactText: true })))
assert.deepEqual(result.hits, [{ entryId: 'e1', start: 7, end: 11 }])
assert.equal('🙂 The girl'.slice(7, 11), 'girl')

analyzer.setStructure(JSON.stringify([{ id: 'b', entryIds: ['e3'] }, { id: 'a', entryIds: ['e1', 'e2'] }]))
const firstUse = JSON.parse(analyzer.firstUse())
assert.ok(firstUse.e3.includes('puella'))
assert.ok(JSON.parse(analyzer.newInLesson('b')).some((lemma) => lemma.key === 'puella'))

analyzer.setEntry(JSON.stringify(entry('e2', 'student', 'latin', 'Puellas, puellas!')))
const direct = JSON.parse(analyzer.review('e2'))
assert.deepEqual(direct.direct, { text: 'Puellās, puellās!', lemmas: [], changed: true })

const review = JSON.parse(analyzer.review('e3'))
assert.equal(review.direct, null)
const choices = review.words.map((word) => word.preselected)
const applied = JSON.parse(analyzer.applyReview('e3', review.original, JSON.stringify(choices)))
assert.equal(typeof applied.text, 'string')
assert.equal(analyzer.applyReview('e3', 'stale', JSON.stringify(choices)), 'null')
assert.equal(analyzer.review('e1'), 'null')

assert.equal(analyzer.macronize('Puella puellae rosam mihi cum amica dat'), 'Puella puellae rosam mihi cum amīcā dat')
assert.throws(() => analyzer.setEntry('{"id": 1}'))

// Spaced repetition: rosa at 0 s, due at 60 s, flagged on the entry starting then.
analyzer.setProject(
  JSON.stringify({
    l1Name: 'English',
    lessons: [{ id: 'a', entries: [entry('r', 'teacher', 'latin', 'rosa', [], 60), entry('x', 'teacher', 'l1', 'later')] }],
  }),
)
analyzer.setGaps(JSON.stringify({ sameParticipantMs: 0, differentParticipantMs: 0 }))
const flags = JSON.parse(analyzer.spacedRepetition())
assert.equal(flags.length, 1)
assert.deepEqual(flags[0].kind, { type: 'inLesson', lessonId: 'a', entryId: 'x' })
assert.equal(flags[0].lemma.headword, 'rosa')
assert.equal(flags[0].elapsedSeconds, 60)
assert.throws(() => analyzer.setGaps('{'))

analyzer.removeEntry('e3')
assert.ok(JSON.parse(analyzer.search('puellae', '{}')).hits.every((hit) => hit.entryId === 'e2'))
console.log('wasm smoke test passed')
