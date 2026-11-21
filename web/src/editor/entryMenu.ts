// Context menu items for an entry (FEATURES §5): single-entry menu, or the
// multi-selection menu when the clicked entry is part of a selection of
// several.
import type { MenuItem } from '../ui/Menu'
import { formatShortcut, type Keymap } from '../keybindings/keybindings'
import type { EditorServices } from './context'
import { editorExtensions } from './extensions'

export function entryMenuItems(services: EditorServices, entryId: string): MenuItem[] {
  const { store, commands, keymap } = services
  const entry = store.getEntry(entryId)
  if (!entry) return []
  const selection = commands.selection()
  const hint = (action: string) => shortcutHint(keymap, action)
  const items: MenuItem[] = []

  if (selection.length > 1 && selection.includes(entryId)) {
    const entries = selection.map((id) => store.getEntry(id)).filter((e) => e !== undefined)
    const hasTeacher = entries.some((e) => e.participant === 'teacher')
    const hasStudent = entries.some((e) => e.participant === 'student')
    const hasTeacherL1 = entries.some((e) => e.participant === 'teacher' && e.language === 'l1')
    const hasTeacherLatin = entries.some((e) => e.participant === 'teacher' && e.language === 'latin')
    items.push({ label: 'Duplicate selected', shortcut: hint('duplicate_entry'), onSelect: () => commands.duplicate(selection) })
    if (hasTeacher) items.push({ label: 'Move to student →', onSelect: () => commands.changeKind(selection, 'toStudent') })
    if (hasStudent) items.push({ label: '← Move to teacher', onSelect: () => commands.changeKind(selection, 'toTeacher') })
    if (hasTeacherL1) items.push({ label: 'Switch to Latin', onSelect: () => commands.changeKind(selection, 'toLatin') })
    if (hasTeacherLatin) items.push({ label: 'Switch to L1', onSelect: () => commands.changeKind(selection, 'toL1') })
    const contiguous = commands.canMove(selection, -1) || commands.canMove(selection, 1) || isContiguous(services, selection)
    if (contiguous) {
      items.push({ label: 'Move up', shortcut: hint('move_entry_up'), disabled: !commands.canMove(selection, -1), onSelect: () => commands.move(selection, -1) })
      items.push({ label: 'Move down', shortcut: hint('move_entry_down'), disabled: !commands.canMove(selection, 1), onSelect: () => commands.move(selection, 1) })
    }
    items.push({ kind: 'separator' })
    items.push({ label: 'Delete selected', danger: true, onSelect: () => commands.delete(selection) })
  } else {
    const teacher = entry.participant === 'teacher'
    items.push({ label: 'Duplicate', shortcut: hint('duplicate_entry'), onSelect: () => commands.duplicate([entryId]) })
    if (entry.language === 'latin') items.push({ label: 'Auto-macronize', onSelect: () => editorExtensions.autoMacronize(services, entryId) })
    items.push(
      teacher
        ? { label: 'Move to student →', shortcut: hint('move_to_student'), onSelect: () => commands.changeKind([entryId], 'toStudent') }
        : { label: '← Move to teacher', shortcut: hint('move_to_teacher'), onSelect: () => commands.changeKind([entryId], 'toTeacher') },
    )
    if (teacher) {
      items.push({
        label: entry.language === 'l1' ? 'Switch to Latin' : 'Switch to L1',
        shortcut: hint('switch_language'),
        onSelect: () => commands.changeKind([entryId], 'toggleLanguage'),
      })
    }
    if (store.getStructure().flat.length > 1 || store.getStructure().lessons.length > 1) {
      items.push({ label: 'Move up', shortcut: hint('move_entry_up'), disabled: !commands.canMove([entryId], -1), onSelect: () => commands.move([entryId], -1) })
      items.push({ label: 'Move down', shortcut: hint('move_entry_down'), disabled: !commands.canMove([entryId], 1), onSelect: () => commands.move([entryId], 1) })
    }
    items.push({ kind: 'separator' })
    items.push({ label: 'Delete', danger: true, onSelect: () => commands.delete([entryId]) })
  }

  const extra = editorExtensions.entryMenuItems.flatMap((contribute) => contribute({ services, entry, selection }))
  if (extra.length) items.splice(items.length - 2, 0, { kind: 'separator' }, ...extra)
  return items
}

function isContiguous(services: EditorServices, ids: string[]): boolean {
  const flat = services.store.getStructure().flat
  const positions = ids.map((id) => flat.indexOf(id)).sort((a, b) => a - b)
  return positions.length > 0 && positions[positions.length - 1] - positions[0] + 1 === positions.length
}

function shortcutHint(keymap: Keymap, action: string): string | undefined {
  const shortcut = keymap[action]
  return shortcut ? formatShortcut(shortcut) : undefined
}
