import { describe, expect, it } from 'vitest'
import {
  actionsFor,
  checkAssignment,
  customBindings,
  defaultKeymap,
  formatShortcut,
  resolveKeymap,
  shortcutFromEvent,
} from './keybindings'

const key = (k: string, mods: Partial<{ ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean }> = {}, code = '') => ({
  key: k,
  code,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
})

describe('shortcutFromEvent', () => {
  it('maps Ctrl to Mod off macOS and ⌘ to Mod on macOS', () => {
    expect(shortcutFromEvent(key('z', { ctrlKey: true }), false)).toBe('Mod+Z')
    expect(shortcutFromEvent(key('Z', { ctrlKey: true, shiftKey: true }), false)).toBe('Mod+Shift+Z')
    expect(shortcutFromEvent(key('z', { metaKey: true }), true)).toBe('Mod+Z')
    expect(shortcutFromEvent(key('z', { ctrlKey: true }), true)).toBe('Ctrl+Z')
    expect(shortcutFromEvent(key('ArrowUp', { metaKey: true, shiftKey: true }), true)).toBe('Mod+Shift+ArrowUp')
  })

  it('ignores bare modifiers and the Super key off macOS', () => {
    expect(shortcutFromEvent(key('Control', { ctrlKey: true }), false)).toBeNull()
    expect(shortcutFromEvent(key('z', { metaKey: true }), false)).toBeNull()
  })

  it('uses the physical key when Option changes the character', () => {
    expect(shortcutFromEvent(key('˚', { altKey: true, metaKey: true }, 'KeyK'), true)).toBe('Mod+Alt+K')
    expect(shortcutFromEvent(key(' '), false)).toBe('Space')
  })

  it('formats shortcuts per platform', () => {
    expect(formatShortcut('Mod+Shift+ArrowUp', false)).toBe('Ctrl+Shift+↑')
    expect(formatShortcut('Mod+Shift+ArrowUp', true)).toBe('⌘⇧↑')
    expect(formatShortcut(null, false)).toBe('None')
  })
})

describe('validation', () => {
  const keymap = defaultKeymap()

  it('requires Ctrl/Cmd for main actions except Escape for stop editing / deselect', () => {
    expect(checkAssignment(keymap, 'edit_entry', 'E')).toBe('Use Ctrl or Command with a key.')
    expect(checkAssignment(keymap, 'edit_entry', 'Escape')).toBe('Use Ctrl or Command with a key.')
    expect(checkAssignment(keymap, 'stop_editing', 'Escape')).toBeNull()
    expect(checkAssignment(keymap, 'edit_entry', 'Ctrl+E')).toBeNull()
  })

  it('allows bare keys for recorder actions but not Escape', () => {
    expect(checkAssignment(keymap, 'recorder_record', 'Q')).toBeNull()
    expect(checkAssignment(keymap, 'recorder_record', 'Escape')).toBe('Use Ctrl or Command with a key.')
  })

  it('detects conflicts by scope', () => {
    // main vs main
    expect(checkAssignment(keymap, 'edit_entry', 'Mod+D')).toBe('That shortcut is already assigned.')
    // shared vs anything
    expect(checkAssignment(keymap, 'recorder_record', 'Mod+Z')).toBe('That shortcut is already assigned.')
    // main vs recorder: allowed
    const custom = { ...keymap, recorder_record: 'Mod+E' }
    expect(checkAssignment(custom, 'duplicate_entry', 'Mod+E')).toBe('That shortcut is already assigned.')
    expect(checkAssignment(keymap, 'recorder_record', 'Mod+E')).toBeNull()
  })

  it('rejects browser-reserved and fixed shortcuts', () => {
    expect(checkAssignment(keymap, 'edit_entry', 'Mod+W')).toMatch(/Browsers/)
    expect(checkAssignment(keymap, 'edit_entry', 'Mod+S')).toBe('That shortcut is reserved.')
  })

  it('resolves stored bindings over defaults and stores only differences', () => {
    const resolved = resolveKeymap({ keybindings: { edit_entry: 'Mod+Shift+E', undo: null, redo: 'E' } })
    expect(resolved.edit_entry).toBe('Mod+Shift+E')
    expect(resolved.undo).toBeNull()
    expect(resolved.redo).toBe('Mod+Shift+Z') // invalid stored value ignored
    expect(customBindings(resolved)).toEqual({ edit_entry: 'Mod+Shift+E', undo: null })
    expect(actionsFor(resolved, 'Escape', ['main', 'shared'])).toEqual(['stop_editing', 'deselect_entries'])
  })
})
