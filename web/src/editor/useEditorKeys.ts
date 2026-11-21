// Editor keyboard shortcuts (FEATURES §14): the user's keymap, exact
// modifier matching, off while dialogs, menus or key capture are open
// (except undo/redo inside dialogs that edit the document).
import { useEffect } from 'react'
import { useNavigate } from 'react-router'
import { actionsFor, FIXED_SHORTCUTS, shortcutFromEvent, type ActionScope } from '../keybindings/keybindings'
import { getLayers } from '../ui/overlay'
import type { EditorServices } from './context'

function isTextField(element: Element | null): element is HTMLElement {
  if (!(element instanceof HTMLElement)) return false
  if (element.isContentEditable) return true
  if (element instanceof HTMLTextAreaElement) return true
  return element instanceof HTMLInputElement && !['checkbox', 'radio', 'button', 'submit', 'range', 'color', 'file'].includes(element.type)
}

/** Run the editor action bound to a shortcut. Returns whether it was handled. */
export function runAction(services: EditorServices, action: string): void {
  const { commands } = services
  switch (action) {
    case 'undo':
      return commands.undo()
    case 'redo':
    case 'redo_alternative':
      return commands.redo()
    case 'edit_entry':
      return commands.editSelected()
    case 'stop_editing':
      return commands.stopEditing()
    case 'deselect_entries':
      return commands.deselect()
    case 'select_previous':
      return commands.selectAdjacent(false)
    case 'select_next':
      return commands.selectAdjacent(true)
    case 'add_teacher_l1':
      return commands.add('teacher', 'l1')
    case 'add_teacher_latin':
      return commands.add('teacher', 'latin')
    case 'add_student_latin':
      return commands.add('student', 'latin')
    case 'duplicate_entry':
    case 'switch_language':
    case 'move_to_student':
    case 'move_to_teacher':
    case 'move_entry_up':
    case 'move_entry_down':
      return commands.entryShortcut(action)
  }
}

export function useEditorKeys(services: EditorServices | null): void {
  const navigate = useNavigate()
  useEffect(() => {
    if (!services) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return
      const shortcut = shortcutFromEvent(event)
      if (!shortcut) return
      const layers = getLayers()
      const fixed = FIXED_SHORTCUTS[shortcut]
      if (fixed) {
        if (layers.length > 0) return
        event.preventDefault()
        if (fixed === 'save') services.commands.save()
        else navigate('/')
        return
      }
      const historyOnly = layers.length > 0
      if (historyOnly && !layers.every((layer) => layer.kind === 'dialog' && layer.allowHistory)) return
      const scopes: ActionScope[] = historyOnly ? ['shared'] : ['shared', 'main']
      let actions = actionsFor(services.keymap, shortcut, scopes)
      if (actions.length === 0) return

      const target = document.activeElement
      if (isTextField(target)) {
        const docField = target.closest('[data-doc-field]') !== null
        // Plain fields (search boxes…) keep their own undo, Escape and keys.
        if (!docField) return
        // Word-wise caret movement wins over moving entries while typing.
        if (/\+Arrow(Left|Right)$/.test(shortcut)) return
      }
      const editing = services.ui.getState().editing !== null
      actions = actions.filter((action) => (action === 'stop_editing' ? editing : action === 'deselect_entries' ? !editing : true))
      if (actions.length === 0) return
      event.preventDefault()
      runAction(services, actions[0])
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [services, navigate])
}
