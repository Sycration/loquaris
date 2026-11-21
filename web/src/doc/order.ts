// Fractional order keys. Lessons and entries are sorted by `order` compared as
// plain strings (never localeCompare), ties broken by ID. Moves only rewrite
// `order` (and `lesson`), so concurrent moves never duplicate or lose items.
import { generateNKeysBetween } from 'fractional-indexing'

export interface Ordered {
  id: string
  order: string
}

export function compareOrdered(a: Ordered, b: Ordered): number {
  if (a.order !== b.order) return a.order < b.order ? -1 : 1
  if (a.id !== b.id) return a.id < b.id ? -1 : 1
  return 0
}

export function sortOrdered<T extends Ordered>(items: T[]): T[] {
  return items.sort(compareOrdered)
}

export interface PlanItem {
  id: string
  /** The item's current key, or null for an item new to this sequence (a
   * fresh insert, or an entry arriving from another lesson). */
  order: string | null
}

/**
 * Assign order keys so that `sequence` sorts in the given order, changing as
 * few keys as possible: the longest run of items whose current keys already
 * increase strictly is kept, and the rest get new keys between their kept
 * neighbours. Items in `prefer` are never kept, so the item the user moved is
 * the one that changes (which composes best with concurrent moves).
 *
 * Returns the new key of every item that needs one.
 */
export function planOrder(sequence: PlanItem[], prefer: ReadonlySet<string> = new Set()): Map<string, string> {
  const keep = keptIndices(sequence, prefer)
  const result = new Map<string, string>()
  try {
    let previous: string | null = null
    let run: number[] = []
    const flush = (next: string | null) => {
      if (run.length === 0) return
      const keys = generateNKeysBetween(previous, next, run.length)
      run.forEach((index, i) => result.set(sequence[index].id, keys[i]))
      run = []
    }
    sequence.forEach((item, index) => {
      if (keep.has(index)) {
        flush(item.order)
        previous = item.order
      } else {
        run.push(index)
      }
    })
    flush(null)
    return result
  } catch {
    // A key that fractional-indexing rejects (e.g. written by a buggy
    // client): renumber the whole sequence.
    const keys = generateNKeysBetween(null, null, sequence.length)
    const all = new Map<string, string>()
    sequence.forEach((item, i) => {
      if (item.order !== keys[i]) all.set(item.id, keys[i])
    })
    return all
  }
}

/** Indices of a longest subsequence with strictly increasing keys
 * (patience sorting, O(n log n)). */
function keptIndices(sequence: PlanItem[], prefer: ReadonlySet<string>): Set<number> {
  const candidates: number[] = []
  sequence.forEach((item, index) => {
    if (item.order !== null && item.order !== '' && !prefer.has(item.id)) candidates.push(index)
  })
  // tails[k]: index (into sequence) of the smallest tail of an increasing run of length k+1.
  const tails: number[] = []
  const parent = new Map<number, number>()
  for (const index of candidates) {
    const key = sequence[index].order!
    let lo = 0
    let hi = tails.length
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (sequence[tails[mid]].order! < key) lo = mid + 1
      else hi = mid
    }
    if (lo > 0) parent.set(index, tails[lo - 1])
    tails[lo] = index
  }
  const kept = new Set<number>()
  let cursor: number | undefined = tails[tails.length - 1]
  while (cursor !== undefined) {
    kept.add(cursor)
    cursor = parent.get(cursor)
  }
  return kept
}

/** Keys for `count` new items appended after `last` (or at the start). */
export function keysAfter(last: string | null, count: number): string[] {
  try {
    return generateNKeysBetween(last || null, null, count)
  } catch {
    return generateNKeysBetween(null, null, count)
  }
}
