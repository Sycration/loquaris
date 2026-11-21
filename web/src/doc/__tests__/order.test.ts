import { describe, expect, it } from 'vitest'
import { generateNKeysBetween } from 'fractional-indexing'
import { compareOrdered, planOrder } from '../order'

function apply(sequence: { id: string; order: string | null }[], keys: Map<string, string>) {
  return sequence.map((item) => ({ id: item.id, order: keys.get(item.id) ?? item.order! }))
}

function sorted(items: { id: string; order: string }[]) {
  return [...items].sort(compareOrdered).map((item) => item.id)
}

describe('order keys', () => {
  it('compares as plain strings with ties broken by id', () => {
    expect(compareOrdered({ id: 'b', order: 'a0' }, { id: 'a', order: 'a0' })).toBe(1)
    // Plain compare: uppercase sorts before lowercase, unlike localeCompare.
    expect(compareOrdered({ id: 'x', order: 'Zz' }, { id: 'y', order: 'a0' })).toBe(-1)
  })

  it('keeps already ordered keys and fills in new items', () => {
    const [k0, k1, k2] = generateNKeysBetween(null, null, 3)
    const sequence = [
      { id: 'a', order: k0 },
      { id: 'new', order: null },
      { id: 'b', order: k1 },
      { id: 'c', order: k2 },
    ]
    const keys = planOrder(sequence)
    expect([...keys.keys()]).toEqual(['new'])
    expect(sorted(apply(sequence, keys))).toEqual(['a', 'new', 'b', 'c'])
  })

  it('changes the preferred (moved) item rather than its neighbours', () => {
    const [k0, k1, k2] = generateNKeysBetween(null, null, 3)
    // c moved to the front.
    const sequence = [
      { id: 'c', order: k2 },
      { id: 'a', order: k0 },
      { id: 'b', order: k1 },
    ]
    const keys = planOrder(sequence, new Set(['c']))
    expect([...keys.keys()]).toEqual(['c'])
    expect(sorted(apply(sequence, keys))).toEqual(['c', 'a', 'b'])
  })

  it('re-keys tied items so an insert between them is possible', () => {
    const sequence = [
      { id: 'a', order: 'a0' },
      { id: 'new', order: null },
      { id: 'b', order: 'a0' },
    ]
    const keys = planOrder(sequence)
    expect(sorted(apply(sequence, keys))).toEqual(['a', 'new', 'b'])
  })

  it('renumbers everything when a key is invalid', () => {
    const sequence = [
      { id: 'a', order: 'not a key' },
      { id: 'new', order: null },
      { id: 'b', order: 'zzzz' },
    ]
    const keys = planOrder(sequence)
    expect(sorted(apply(sequence, keys))).toEqual(['a', 'new', 'b'])
  })
})
