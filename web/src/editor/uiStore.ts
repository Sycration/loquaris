// Per-user editor UI state that is not part of the document: selection,
// which entry is being edited, the active lesson, collapsed lessons, and
// one-shot focus / scroll requests. Components read slices through
// useUi(selector), so a selection change re-renders only cards whose
// selected flag changed.
import { useCallback, useSyncExternalStore } from 'react'
import { EMPTY_SELECTION, type SelectionState } from './selection'

export interface Request {
  kind: 'entry' | 'lesson'
  id: string
  /** Smoothly centre instead of scrolling the least distance. */
  center?: boolean
  nonce: number
}

export interface ContextMenuState {
  entryId: string
  x: number
  y: number
}

export interface UiState {
  selection: SelectionState
  /** The entry whose text is being edited. */
  editing: string | null
  /** Target of new entries and the side panel. */
  activeLesson: string | null
  collapsed: ReadonlySet<string>
  focusRequest: Request | null
  scrollRequest: Request | null
  contextMenu: ContextMenuState | null
}

type Listener = () => void

let nonce = 0
export function nextNonce(): number {
  return ++nonce
}

export class UiStore {
  private state: UiState
  private listeners = new Set<Listener>()
  private storageKey: string | null

  constructor(storageKey: string | null = null) {
    this.storageKey = storageKey
    this.state = {
      selection: EMPTY_SELECTION,
      editing: null,
      activeLesson: null,
      collapsed: new Set(loadCollapsed(storageKey)),
      focusRequest: null,
      scrollRequest: null,
      contextMenu: null,
    }
  }

  getState = (): UiState => this.state

  setState(patch: Partial<UiState>): void {
    let changed = false
    for (const key of Object.keys(patch) as (keyof UiState)[]) {
      if (patch[key] !== this.state[key]) changed = true
    }
    if (!changed) return
    const previous = this.state
    this.state = { ...this.state, ...patch }
    if (patch.collapsed && patch.collapsed !== previous.collapsed) saveCollapsed(this.storageKey, patch.collapsed)
    for (const listener of this.listeners) listener()
  }

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }
}

export function useUi<T>(store: UiStore, selector: (state: UiState) => T): T {
  // The selector may be an inline function; read through a stable callback.
  const get = useCallback(() => selector(store.getState()), [store, selector])
  return useSyncExternalStore(store.subscribe, get)
}

function loadCollapsed(key: string | null): string[] {
  if (!key) return []
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? '[]')
    return Array.isArray(value) ? value.filter((item) => typeof item === 'string') : []
  } catch {
    return []
  }
}

function saveCollapsed(key: string | null, collapsed: ReadonlySet<string>) {
  if (!key) return
  try {
    if (collapsed.size === 0) localStorage.removeItem(key)
    else localStorage.setItem(key, JSON.stringify(Array.from(collapsed)))
  } catch {
    // Storage unavailable (private mode, quota): collapse state just isn't kept.
  }
}
