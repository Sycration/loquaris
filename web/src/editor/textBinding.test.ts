import { describe, expect, it } from 'vitest'
import * as Y from 'yjs'
import { LOCAL_ORIGIN } from '../doc/schema'
import { TextareaBinding } from './textBinding'
import { applyChange, diffText, transformChange, transformIndex, transformSelection } from './textDiff'

describe('diffText', () => {
  it('finds the minimal replacement', () => {
    expect(diffText('puella', 'puellā')).toEqual({ index: 5, deleteCount: 1, insert: 'ā' })
    expect(diffText('abc', 'abc')).toBeNull()
    expect(diffText('', 'x')).toEqual({ index: 0, deleteCount: 0, insert: 'x' })
    expect(diffText('hello world', 'hello')).toEqual({ index: 5, deleteCount: 6, insert: '' })
  })

  it('uses the cursor to place insertions among repeated characters', () => {
    // Typing "a" at position 1 of "aa".
    expect(diffText('aa', 'aaa', 2)).toEqual({ index: 1, deleteCount: 0, insert: 'a' })
    // Without the cursor, the common suffix wins and the insertion goes first.
    expect(diffText('aa', 'aaa')).toEqual({ index: 0, deleteCount: 0, insert: 'a' })
    // Backspace at position 1 of "aaa".
    expect(diffText('aaa', 'aa', 0)).toEqual({ index: 0, deleteCount: 1, insert: '' })
  })

  it('never splits surrogate pairs', () => {
    // 😀 = 😀, 😁 = 😁: they share the high surrogate.
    const change = diffText('a😀b', 'a😁b')!
    expect(change).toEqual({ index: 1, deleteCount: 2, insert: '😁' })
    expect(applyChange('a😀b', change)).toBe('a😁b')
  })

  it('round-trips random edits', () => {
    const alphabet = 'ab ā'
    for (let i = 0; i < 300; i++) {
      const random = () => Array.from({ length: Math.floor(Math.random() * 8) }, () => alphabet[Math.floor(Math.random() * alphabet.length)]).join('')
      const before = random()
      const after = random()
      const change = diffText(before, after, Math.floor(Math.random() * (after.length + 1)))
      expect(change ? applyChange(before, change) : before).toBe(after)
    }
  })
})

describe('transformIndex', () => {
  it('shifts through insertions and deletions before the index', () => {
    expect(transformIndex(5, [{ insert: 'abc' }])).toBe(8)
    expect(transformIndex(5, [{ retain: 6 }, { insert: 'abc' }])).toBe(5)
    expect(transformIndex(5, [{ retain: 1 }, { delete: 2 }])).toBe(3)
    // A deletion spanning the index collapses it to the deletion point.
    expect(transformIndex(5, [{ retain: 3 }, { delete: 4 }])).toBe(3)
  })

  it('sticks right at an insertion exactly at the index unless asked', () => {
    expect(transformIndex(2, [{ retain: 2 }, { insert: 'xy' }])).toBe(4)
    expect(transformIndex(2, [{ retain: 2 }, { insert: 'xy' }], true)).toBe(2)
  })

  it('keeps text inserted right after a selection outside it', () => {
    expect(transformSelection({ start: 1, end: 3 }, [{ retain: 3 }, { insert: '!!' }])).toMatchObject({ start: 1, end: 3 })
    expect(transformSelection({ start: 1, end: 3 }, [{ insert: '>' }])).toMatchObject({ start: 2, end: 4 })
  })

  it('transforms an older change through a concurrent delta', () => {
    // Base "hello", local replaced "ll" (2..4) with "LL"; remote inserted "> " at 0.
    expect(transformChange({ index: 2, deleteCount: 2, insert: 'LL' }, [{ insert: '> ' }])).toEqual({
      index: 4,
      deleteCount: 2,
      insert: 'LL',
    })
  })

  it('agrees with Y.Text deltas', () => {
    const doc = new Y.Doc()
    const text = doc.getText('t')
    text.insert(0, 'The quick brown fox')
    let delta: unknown[] = []
    text.observe((event) => {
      delta = event.delta
    })
    doc.transact(() => {
      text.delete(4, 6) // "quick "
      text.insert(0, '>> ')
    })
    const fox = 'The quick brown fox'.indexOf('fox')
    expect(text.toString().slice(transformIndex(fox, delta as never))).toBe('fox')
  })
})

function setup(initial = '') {
  const doc = new Y.Doc()
  const ytext = doc.getText('t')
  ytext.insert(0, initial)
  const textarea = document.createElement('textarea')
  document.body.append(textarea)
  const values: string[] = []
  const binding = new TextareaBinding(textarea, ytext, { onValue: (value) => values.push(value) })
  const type = (value: string, caret = value.length) => {
    textarea.value = value
    textarea.setSelectionRange(caret, caret)
    textarea.dispatchEvent(new InputEvent('input', { bubbles: true }))
  }
  return { doc, ytext, textarea, binding, type, values }
}

describe('TextareaBinding', () => {
  it('shows the initial text and applies local input as minimal local-origin changes', () => {
    const { doc, ytext, textarea, type } = setup('ave')
    expect(textarea.value).toBe('ave')
    const origins: unknown[] = []
    doc.on('afterTransaction', (tr) => origins.push(tr.origin))
    type('salve', 0)
    expect(ytext.toString()).toBe('salve')
    expect(origins).toEqual([LOCAL_ORIGIN])
  })

  it('keeps the local caret in place across remote edits', () => {
    const { ytext, textarea } = setup('salve magister')
    textarea.focus()
    textarea.setSelectionRange(6, 6) // before "magister"
    ytext.insert(0, 'O ')
    expect(textarea.value).toBe('O salve magister')
    expect(textarea.selectionStart).toBe(8)
    ytext.delete(textarea.value.indexOf('magister'), 8)
    expect(textarea.selectionStart).toBe(8)
  })

  it('defers remote edits during IME composition and merges afterwards', () => {
    const { ytext, textarea } = setup('ab')
    textarea.focus()
    textarea.dispatchEvent(new CompositionEvent('compositionstart'))
    textarea.value = 'abか'
    textarea.setSelectionRange(3, 3)
    textarea.dispatchEvent(new InputEvent('input', { isComposing: true }))
    // Remote edit while composing: the textarea is not touched.
    ytext.insert(0, 'X')
    expect(textarea.value).toBe('abか')
    expect(ytext.toString()).toBe('Xab')
    textarea.dispatchEvent(new CompositionEvent('compositionend'))
    expect(ytext.toString()).toBe('Xabか')
    expect(textarea.value).toBe('Xabか')
    expect(textarea.selectionStart).toBe(4)
  })

  it('reports value changes and stops after destroy', () => {
    const { ytext, textarea, binding, values } = setup('a')
    ytext.insert(1, 'b')
    expect(values).toEqual(['ab'])
    binding.destroy()
    ytext.insert(2, 'c')
    expect(textarea.value).toBe('ab')
  })

  it('sanitizes input', () => {
    const doc = new Y.Doc()
    const ytext = doc.getText('t')
    const textarea = document.createElement('textarea')
    new TextareaBinding(textarea, ytext, { sanitize: (value) => value.replace(/\n/g, ' ') })
    textarea.value = 'one\ntwo'
    textarea.dispatchEvent(new InputEvent('input'))
    expect(ytext.toString()).toBe('one two')
  })
})
