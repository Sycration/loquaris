// Keyboard shortcuts (FEATURES §14, DESIGN.md "Keybindings").
//
// A shortcut is a string such as "Mod+Shift+ArrowUp": modifiers in the order
// Mod, Ctrl, Alt, Shift, then the key. "Mod" is Ctrl on Linux/Windows and ⌘
// on macOS; "Ctrl" appears only on macOS, where Control is a separate key.
// Matching is exact: Mod+Shift+Z never triggers Mod+Z.
import type { UserSettings } from '../api/types'

export type ActionScope = 'shared' | 'main' | 'recorder'

export interface ActionDef {
  id: string
  label: string
  scope: ActionScope
  default: string | null
}

export const ACTIONS: readonly ActionDef[] = [
  { id: 'undo', label: 'Undo', scope: 'shared', default: 'Mod+Z' },
  { id: 'redo', label: 'Redo', scope: 'shared', default: 'Mod+Shift+Z' },
  { id: 'redo_alternative', label: 'Redo (alternative)', scope: 'shared', default: 'Mod+Y' },
  { id: 'edit_entry', label: 'Edit selected entry', scope: 'main', default: 'Mod+E' },
  { id: 'duplicate_entry', label: 'Duplicate selected entries', scope: 'main', default: 'Mod+D' },
  { id: 'stop_editing', label: 'Stop editing entry', scope: 'main', default: 'Escape' },
  { id: 'deselect_entries', label: 'Deselect entries', scope: 'main', default: 'Escape' },
  { id: 'select_previous', label: 'Select previous entry', scope: 'main', default: 'Mod+ArrowUp' },
  { id: 'select_next', label: 'Select next entry', scope: 'main', default: 'Mod+ArrowDown' },
  { id: 'add_teacher_l1', label: 'Add teacher L1', scope: 'main', default: 'Mod+J' },
  { id: 'add_teacher_latin', label: 'Add teacher Latin', scope: 'main', default: 'Mod+K' },
  { id: 'add_student_latin', label: 'Add student Latin', scope: 'main', default: 'Mod+L' },
  { id: 'switch_language', label: 'Switch entry language', scope: 'main', default: 'Mod+R' },
  { id: 'move_to_student', label: 'Move to student', scope: 'main', default: 'Mod+ArrowRight' },
  { id: 'move_to_teacher', label: 'Move to teacher', scope: 'main', default: 'Mod+ArrowLeft' },
  { id: 'move_entry_up', label: 'Move entry up', scope: 'main', default: 'Mod+Shift+ArrowUp' },
  { id: 'move_entry_down', label: 'Move entry down', scope: 'main', default: 'Mod+Shift+ArrowDown' },
  { id: 'recorder_play_pause', label: 'Play / pause', scope: 'recorder', default: 'Space' },
  { id: 'recorder_record', label: 'Start / stop recording', scope: 'recorder', default: 'R' },
  { id: 'recorder_stop_playback', label: 'Stop playback', scope: 'recorder', default: 'S' },
  { id: 'recorder_seek_back', label: 'Seek backward 5 seconds', scope: 'recorder', default: 'ArrowLeft' },
  { id: 'recorder_seek_forward', label: 'Seek forward 5 seconds', scope: 'recorder', default: 'ArrowRight' },
  { id: 'recorder_seek_start', label: 'Seek to beginning', scope: 'recorder', default: 'Home' },
  { id: 'recorder_seek_end', label: 'Seek to end', scope: 'recorder', default: 'End' },
]

export type ActionId = (typeof ACTIONS)[number]['id']

const ACTION_BY_ID = new Map(ACTIONS.map((action) => [action.id, action]))

export function actionDef(id: string): ActionDef | undefined {
  return ACTION_BY_ID.get(id)
}

/** Fixed shortcuts that are not rebindable: Mod+S shows "All changes saved"
 * and Mod+O opens the project list (DESIGN.md, "Changes from the desktop
 * app"). */
export const FIXED_SHORTCUTS: Readonly<Record<string, string>> = {
  'Mod+S': 'save',
  'Mod+O': 'open_projects',
}

/** Shortcuts browsers keep for themselves; pages never receive them. */
const BROWSER_RESERVED = new Set([
  'Mod+N',
  'Mod+Shift+N',
  'Mod+T',
  'Mod+Shift+T',
  'Mod+W',
  'Mod+Shift+W',
  'Mod+Q',
  'Mod+Tab',
  'Mod+Shift+Tab',
  'Mod+PageUp',
  'Mod+PageDown',
  'Mod+H',
  'Mod+M',
])

export const isMac = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)

const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta', 'AltGraph', 'OS', 'Hyper', 'Super', 'CapsLock', 'Fn'])

/** The key part of a shortcut for a keyboard event, or null for a bare
 * modifier press. Letters and digits are layout-aware where possible. */
export function eventKey(event: Pick<KeyboardEvent, 'key' | 'code'>): string | null {
  const { key, code } = event
  if (MODIFIER_KEYS.has(key) || key === 'Dead' || key === 'Unidentified') {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3)
    if (/^Digit[0-9]$/.test(code)) return code.slice(5)
    return null
  }
  if (key === ' ') return 'Space'
  if (key.length === 1) {
    if (/^[a-z]$/i.test(key)) return key.toUpperCase()
    // Option/Alt and Shift change the character on many layouts; fall back
    // to the physical key for letters and digits.
    if (/^Key[A-Z]$/.test(code)) return code.slice(3)
    if (/^Digit[0-9]$/.test(code)) return code.slice(5)
    return key
  }
  return key
}

export interface ModifierState {
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
  shiftKey: boolean
}

/** The shortcut string for a keyboard event, or null for a modifier alone
 * (or the Windows/Super key, which we don't use). */
export function shortcutFromEvent(event: ModifierState & Pick<KeyboardEvent, 'key' | 'code'>, mac = isMac): string | null {
  const key = eventKey(event)
  if (!key) return null
  if (!mac && event.metaKey) return null
  const parts: string[] = []
  if (mac ? event.metaKey : event.ctrlKey) parts.push('Mod')
  if (mac && event.ctrlKey) parts.push('Ctrl')
  if (event.altKey) parts.push('Alt')
  if (event.shiftKey) parts.push('Shift')
  parts.push(key)
  return parts.join('+')
}

const KEY_LABELS: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Escape: 'Esc',
  Space: 'Space',
}

/** Human-readable form, e.g. "Ctrl+Shift+↑" or "⌘⇧↑". */
export function formatShortcut(shortcut: string | null, mac = isMac): string {
  if (!shortcut) return 'None'
  const parts = shortcut.split('+')
  const key = parts.pop()!
  const label = KEY_LABELS[key] ?? key
  if (mac) {
    const symbols: Record<string, string> = { Mod: '⌘', Ctrl: '⌃', Alt: '⌥', Shift: '⇧' }
    return parts.map((part) => symbols[part] ?? part).join('') + label
  }
  return [...parts.map((part) => (part === 'Mod' ? 'Ctrl' : part)), label].join('+')
}

function hasMod(shortcut: string): boolean {
  const parts = shortcut.split('+')
  return parts.includes('Mod') || parts.includes('Ctrl')
}

/** Whether two actions may not share a shortcut (FEATURES §14): shared
 * conflicts with everything; main and recorder don't conflict with each
 * other; Stop editing and Deselect may share. */
export function overlaps(a: ActionDef, b: ActionDef): boolean {
  const pair = new Set([a.id, b.id])
  if (pair.has('stop_editing') && pair.has('deselect_entries') && a.id !== b.id) return false
  return a.scope === 'shared' || b.scope === 'shared' || a.scope === b.scope
}

export type Keymap = Readonly<Record<string, string | null>>

export function defaultKeymap(): Keymap {
  return Object.fromEntries(ACTIONS.map((action) => [action.id, action.default]))
}

/** Defaults overlaid with the user's bindings. Invalid stored bindings fall
 * back to the default. */
export function resolveKeymap(settings: UserSettings | undefined): Keymap {
  const map: Record<string, string | null> = { ...defaultKeymap() }
  const custom = settings?.keybindings ?? {}
  for (const action of ACTIONS) {
    if (!(action.id in custom)) continue
    const value = custom[action.id]
    if (value === null || (typeof value === 'string' && validateShortcut(action, value) === null)) map[action.id] = value
  }
  return map
}

/** Why `shortcut` cannot be used for `action` on its own, or null. */
export function validateShortcut(action: ActionDef, shortcut: string): string | null {
  const key = shortcut.split('+').pop()
  const bare = !hasMod(shortcut)
  if (BROWSER_RESERVED.has(shortcut)) return 'Browsers don’t let web pages use that shortcut.'
  if (FIXED_SHORTCUTS[shortcut]) return 'That shortcut is reserved.'
  if (action.scope === 'recorder') {
    return key === 'Escape' ? 'Use Ctrl or Command with a key.' : null
  }
  if (shortcut === 'Escape' && (action.id === 'stop_editing' || action.id === 'deselect_entries')) return null
  if (bare) return 'Use Ctrl or Command with a key.'
  return null
}

/** Why `shortcut` cannot be assigned to `actionId` in `keymap`, or null. */
export function checkAssignment(keymap: Keymap, actionId: string, shortcut: string): string | null {
  const action = actionDef(actionId)
  if (!action) return 'Unknown action.'
  const invalid = validateShortcut(action, shortcut)
  if (invalid) return invalid
  for (const other of ACTIONS) {
    if (other.id === actionId || keymap[other.id] !== shortcut) continue
    if (overlaps(action, other)) return 'That shortcut is already assigned.'
  }
  return null
}

/** The stored form: only bindings that differ from the defaults. */
export function customBindings(keymap: Keymap): Record<string, string | null> {
  const result: Record<string, string | null> = {}
  for (const action of ACTIONS) {
    if (keymap[action.id] !== action.default) result[action.id] = keymap[action.id] ?? null
  }
  return result
}

/** Actions of the given scopes bound to `shortcut`. */
export function actionsFor(keymap: Keymap, shortcut: string, scopes: readonly ActionScope[]): string[] {
  return ACTIONS.filter((action) => scopes.includes(action.scope) && keymap[action.id] === shortcut).map((a) => a.id)
}
