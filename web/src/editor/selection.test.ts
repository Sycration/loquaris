import { describe, expect, it } from 'vitest'
import { adjacentEntry, clickSelection, EMPTY_SELECTION, pruneSelection, selectedIds, selectionSearchQuery, selectOnly } from './selection'

const flat = ['a', 'b', 'c', 'd', 'e']
const none = { mod: false, alt: false, shift: false }

describe('selection', () => {
  it('click selects only, Mod-click adds, Alt-click removes', () => {
    let s = clickSelection(EMPTY_SELECTION, 'b', none, flat)
    expect(selectedIds(s, flat)).toEqual(['b'])
    s = clickSelection(s, 'd', { ...none, mod: true }, flat)
    expect(selectedIds(s, flat)).toEqual(['b', 'd'])
    expect(s.primary).toBe('d')
    // Mod-click adds, never toggles.
    s = clickSelection(s, 'd', { ...none, mod: true }, flat)
    expect(selectedIds(s, flat)).toEqual(['b', 'd'])
    s = clickSelection(s, 'd', { ...none, alt: true }, flat)
    expect(selectedIds(s, flat)).toEqual(['b'])
    expect(s.primary).toBe('b')
  })

  it('Shift-click selects a range from the anchor', () => {
    let s = clickSelection(EMPTY_SELECTION, 'b', none, flat)
    s = clickSelection(s, 'e', { ...none, shift: true }, flat)
    expect(selectedIds(s, flat)).toEqual(['b', 'c', 'd', 'e'])
    s = clickSelection(s, 'a', { ...none, shift: true }, flat)
    expect(selectedIds(s, flat)).toEqual(['a', 'b'])
  })

  it('right click keeps a selection containing the entry', () => {
    const s = clickSelection(clickSelection(selectOnly('a'), 'c', { ...none, mod: true }, flat), 'a', none, flat, true)
    expect(selectedIds(s, flat)).toEqual(['a', 'c'])
    expect(selectedIds(clickSelection(s, 'e', none, flat, true), flat)).toEqual(['e'])
  })

  it('moves to adjacent entries, clamped, or the active lesson ends', () => {
    expect(adjacentEntry(selectOnly('c'), flat, [], true)).toBe('d')
    expect(adjacentEntry(selectOnly('e'), flat, [], true)).toBe('e')
    expect(adjacentEntry(selectOnly('a'), flat, [], false)).toBe('a')
    expect(adjacentEntry(EMPTY_SELECTION, flat, ['c', 'd'], true)).toBe('c')
    expect(adjacentEntry(EMPTY_SELECTION, flat, ['c', 'd'], false)).toBe('d')
  })

  it('prunes deleted entries', () => {
    const s = clickSelection(selectOnly('a'), 'b', { ...none, mod: true }, flat)
    const pruned = pruneSelection(s, (id) => id !== 'b')
    expect(pruned.primary).toBe('a')
    expect(pruneSelection(pruned, () => true)).toBe(pruned)
  })

  it('turns selected text into a search query', () => {
    expect(selectionSearchQuery(' «puellam», ')).toBe('puellam')
    expect(selectionSearchQuery('two\nlines')).toBeNull()
    expect(selectionSearchQuery('...')).toBeNull()
    expect(selectionSearchQuery('x'.repeat(101))).toBeNull()
  })
})
