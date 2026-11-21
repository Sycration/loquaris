// Entry selection (FEATURES §7): a primary entry, extra selected entries and
// a range anchor, all by ID; "script order" is the flat order across lessons.
// Pure functions so the rules are easy to test.

export interface SelectionState {
  primary: string | null
  extra: ReadonlySet<string>
  anchor: string | null
}

export const EMPTY_SELECTION: SelectionState = { primary: null, extra: new Set(), anchor: null }

export interface ClickModifiers {
  /** Ctrl on Linux/Windows, ⌘ on macOS. */
  mod: boolean
  alt: boolean
  shift: boolean
}

/** Every selected entry that still exists, in script order. */
export function selectedIds(state: SelectionState, flat: readonly string[]): string[] {
  if (state.primary === null && state.extra.size === 0) return []
  return flat.filter((id) => id === state.primary || state.extra.has(id))
}

export function isSelected(state: SelectionState, id: string): boolean {
  return state.primary === id || state.extra.has(id)
}

export function selectOnly(id: string): SelectionState {
  return { primary: id, extra: new Set(), anchor: id }
}

/** Select `ids`, with `primary` (if among them) or the last as primary. */
export function selectMany(ids: readonly string[], primary: string | null = null): SelectionState {
  if (ids.length === 0) return EMPTY_SELECTION
  const main = primary !== null && ids.includes(primary) ? primary : ids[ids.length - 1]
  return { primary: main, extra: new Set(ids.filter((id) => id !== main)), anchor: main }
}

/** The selection after clicking `target`. A right click on a selected entry
 * keeps the selection (for the multi-selection menu). */
export function clickSelection(
  state: SelectionState,
  target: string,
  modifiers: ClickModifiers,
  flat: readonly string[],
  rightClick = false,
): SelectionState {
  let selected = new Set(selectedIds(state, flat))
  let anchor = state.anchor
  if (rightClick) {
    if (selected.has(target)) return state
    selected = new Set([target])
    anchor = target
  } else if (modifiers.alt) {
    selected.delete(target)
    if (anchor === target) anchor = lastInOrder(selected, flat)
  } else if (modifiers.shift) {
    const from = anchor !== null && flat.includes(anchor) ? anchor : state.primary !== null && flat.includes(state.primary) ? state.primary : target
    const a = flat.indexOf(from)
    const b = flat.indexOf(target)
    selected = new Set(flat.slice(Math.min(a, b), Math.max(a, b) + 1))
    anchor = from
  } else if (modifiers.mod) {
    selected.add(target)
    anchor = target
  } else {
    selected = new Set([target])
    anchor = target
  }
  const primary = selected.has(target) ? target : lastInOrder(selected, flat)
  if (primary !== null) selected.delete(primary)
  return { primary, extra: selected, anchor }
}

function lastInOrder(ids: ReadonlySet<string>, flat: readonly string[]): string | null {
  for (let i = flat.length - 1; i >= 0; i--) if (ids.has(flat[i])) return flat[i]
  return null
}

/** The entry Mod+↑ / Mod+↓ selects: the previous / next entry across
 * lessons, or with nothing selected the first / last entry of the active
 * lesson (else of the script). */
export function adjacentEntry(
  state: SelectionState,
  flat: readonly string[],
  activeLessonEntries: readonly string[],
  next: boolean,
): string | null {
  if (flat.length === 0) return null
  if (state.primary !== null && flat.includes(state.primary)) {
    const index = flat.indexOf(state.primary)
    return flat[next ? Math.min(index + 1, flat.length - 1) : Math.max(index - 1, 0)]
  }
  if (activeLessonEntries.length > 0) return next ? activeLessonEntries[0] : activeLessonEntries[activeLessonEntries.length - 1]
  return next ? flat[0] : flat[flat.length - 1]
}

/** Drop entries that no longer exist. Returns the same object if unchanged. */
export function pruneSelection(state: SelectionState, exists: (id: string) => boolean): SelectionState {
  const primaryOk = state.primary === null || exists(state.primary)
  const extraOk = Array.from(state.extra).every(exists)
  const anchorOk = state.anchor === null || exists(state.anchor)
  if (primaryOk && extraOk && anchorOk) return state
  const extra = new Set(Array.from(state.extra).filter(exists))
  let primary = primaryOk ? state.primary : null
  if (primary === null && extra.size > 0) {
    primary = Array.from(extra).pop()!
    extra.delete(primary)
  }
  return { primary, extra, anchor: anchorOk ? state.anchor : primary }
}

/** Text selected in a card that isn't being edited, as a search query
 * (FEATURES §4): trimmed of surrounding non-alphanumerics; null if empty,
 * multi-line or longer than 100 characters. */
export function selectionSearchQuery(text: string): string | null {
  if (/[\r\n]/.test(text.trim())) return null
  const trimmed = text.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, '')
  if (!trimmed || trimmed.length > 100) return null
  return trimmed
}
