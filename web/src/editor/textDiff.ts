// Pure helpers for binding a <textarea> to a Y.Text: the minimal edit between
// two strings, and transforming indices through a Y.Text delta. Indices are
// UTF-16 code units, as both the DOM and Y.Text use.

export interface TextChange {
  index: number
  deleteCount: number
  insert: string
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff
}

/**
 * The single replacement turning `before` into `after`. `cursor` is the
 * caret after the edit, which disambiguates repeated characters: typing "a"
 * into "a|a" is an insertion at 1, not at 2. Never splits a surrogate pair.
 */
export function diffText(before: string, after: string, cursor?: number): TextChange | null {
  if (before === after) return null
  const shortest = Math.min(before.length, after.length)
  // The changed region of `after` ends at or after the cursor, so the common
  // suffix cannot extend left of it.
  let suffixLimit = shortest
  if (cursor !== undefined && cursor >= 0 && cursor <= after.length) {
    suffixLimit = Math.min(suffixLimit, after.length - cursor)
  }
  let suffix = 0
  while (
    suffix < suffixLimit &&
    before.charCodeAt(before.length - 1 - suffix) === after.charCodeAt(after.length - 1 - suffix)
  ) {
    suffix++
  }
  let prefix = 0
  const prefixLimit = shortest - suffix
  while (prefix < prefixLimit && before.charCodeAt(prefix) === after.charCodeAt(prefix)) prefix++

  // Keep surrogate pairs whole.
  if (prefix > 0 && isHighSurrogate(before.charCodeAt(prefix - 1))) prefix--
  if (suffix > 0 && isLowSurrogate(before.charCodeAt(before.length - suffix))) suffix--

  return {
    index: prefix,
    deleteCount: before.length - prefix - suffix,
    insert: after.slice(prefix, after.length - suffix),
  }
}

export function applyChange(text: string, change: TextChange): string {
  return text.slice(0, change.index) + change.insert + text.slice(change.index + change.deleteCount)
}

/** One operation of a Y.Text delta (YTextEvent.delta). */
export interface DeltaOp {
  retain?: number
  insert?: string | object
  delete?: number
}

/**
 * Where `index` ends up after `delta` is applied. An insertion exactly at the
 * index pushes it right (like a Y relative position, which sticks to the
 * character after it), unless `stickLeft`.
 */
export function transformIndex(index: number, delta: readonly DeltaOp[], stickLeft = false): number {
  let position = 0 // position in the old text
  let result = index
  for (const op of delta) {
    if (op.retain !== undefined) {
      position += op.retain
    } else if (op.insert !== undefined) {
      const length = typeof op.insert === 'string' ? op.insert.length : 1
      if (position < index || (position === index && !stickLeft)) result += length
    } else if (op.delete !== undefined) {
      if (position < index) result -= Math.min(op.delete, index - position)
      position += op.delete
    }
    if (position > index) break
  }
  return Math.max(0, result)
}

export interface TextSelection {
  start: number
  end: number
  direction?: 'forward' | 'backward' | 'none'
}

export function transformSelection(selection: TextSelection, delta: readonly DeltaOp[]): TextSelection {
  const collapsed = selection.start === selection.end
  const start = transformIndex(selection.start, delta)
  // A collapsed caret moves as one; otherwise the end sticks left so text
  // inserted right after a selection isn't swallowed by it.
  const end = collapsed ? start : Math.max(start, transformIndex(selection.end, delta, true))
  return { start, end, direction: selection.direction }
}

/** Transform a change made against an older text through a delta applied to
 * that text meanwhile (used for IME compositions that span remote edits). */
export function transformChange(change: TextChange, delta: readonly DeltaOp[]): TextChange {
  const start = transformIndex(change.index, delta, true)
  const end = Math.max(start, transformIndex(change.index + change.deleteCount, delta, true))
  return { index: start, deleteCount: end - start, insert: change.insert }
}
