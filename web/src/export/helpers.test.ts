import { describe, expect, it } from 'vitest'
import { filenameFromContentDisposition, incompleteLessons, lessonComplete } from './helpers'

describe('filenameFromContentDisposition', () => {
  it('reads quoted and bare filenames', () => {
    expect(filenameFromContentDisposition('attachment; filename="Lesson 1.pdf"')).toBe('Lesson 1.pdf')
    expect(filenameFromContentDisposition('attachment; filename=a.wav')).toBe('a.wav')
  })
  it('prefers RFC 5987 filename*', () => {
    expect(
      filenameFromContentDisposition(`attachment; filename="fallback.pdf"; filename*=UTF-8''Caes%C4%81r%20I.pdf`),
    ).toBe('Caesār I.pdf')
  })
  it('handles escapes, paths and missing headers', () => {
    expect(filenameFromContentDisposition('attachment; filename="a\\"b.pdf"')).toBe('a"b.pdf')
    expect(filenameFromContentDisposition('attachment; filename="../../x.pdf"')).toBe('x.pdf')
    expect(filenameFromContentDisposition('attachment')).toBeNull()
    expect(filenameFromContentDisposition(null)).toBeNull()
  })
  it('falls back when filename* is malformed', () => {
    expect(filenameFromContentDisposition(`attachment; filename*=UTF-8''%E0%A4%A; filename="ok.pdf"`)).toBe('ok.pdf')
  })
})

describe('completeness', () => {
  const full = { id: 'a', name: 'A', recorded: [true, true] }
  const partial = { id: 'b', name: 'B', recorded: [true, false] }
  it('requires entries and every clip', () => {
    expect(lessonComplete(full)).toBe(true)
    expect(lessonComplete(partial)).toBe(false)
    expect(lessonComplete({ id: 'c', name: 'C', recorded: [] })).toBe(false)
    expect(incompleteLessons([full, partial]).map((l) => l.id)).toEqual(['b'])
  })
})
