// Table of actions and their shortcuts (FEATURES §14): click a shortcut,
// press the new one; Escape cancels; "Restore defaults". Saved in the user's
// settings so bindings follow them between browsers.
import { useEffect, useState } from 'react'
import { Button } from '../ui/Button'
import { useOverlayLayer } from '../ui/overlay'
import {
  ACTIONS,
  checkAssignment,
  customBindings,
  defaultKeymap,
  formatShortcut,
  shortcutFromEvent,
  type ActionDef,
  type Keymap,
} from './keybindings'

export interface KeybindingEditorProps {
  keymap: Keymap
  /** Persist the custom bindings (only those differing from defaults). */
  onSave: (custom: Record<string, string | null>) => Promise<void>
}

export function KeybindingEditor({ keymap, onSave }: KeybindingEditorProps) {
  const [capturing, setCapturing] = useState<string | null>(null)
  const [error, setError] = useState<{ action: string; message: string } | null>(null)
  const [saving, setSaving] = useState(false)
  useOverlayLayer(capturing !== null, 'capture')

  const save = async (next: Keymap) => {
    setSaving(true)
    try {
      await onSave(customBindings(next))
    } finally {
      setSaving(false)
    }
  }

  useEffect(() => {
    if (!capturing) return
    const onKeyDown = (event: KeyboardEvent) => {
      event.preventDefault()
      event.stopPropagation()
      if (event.repeat) return
      const shortcut = shortcutFromEvent(event)
      if (!shortcut) return
      const action = capturing
      if (shortcut === 'Escape' && action !== 'stop_editing' && action !== 'deselect_entries') {
        setCapturing(null)
        setError(null)
        return
      }
      const problem = checkAssignment(keymap, action, shortcut)
      if (problem) {
        setError({ action, message: problem })
        return
      }
      setCapturing(null)
      setError(null)
      void save({ ...keymap, [action]: shortcut })
    }
    window.addEventListener('keydown', onKeyDown, true)
    return () => window.removeEventListener('keydown', onKeyDown, true)
  }, [capturing, keymap]) // save only depends on keymap

  const row = (action: ActionDef) => {
    const isCapturing = capturing === action.id
    const rowError = error?.action === action.id ? error.message : null
    return (
      <tr key={action.id}>
        <th scope="row">{action.label}</th>
        <td>
          <button
            type="button"
            className={['shortcut-button', isCapturing && 'shortcut-button--capturing'].filter(Boolean).join(' ')}
            aria-label={`${action.label}: ${isCapturing ? 'press a shortcut' : formatShortcut(keymap[action.id] ?? null)}. Click to change.`}
            onClick={() => {
              setError(null)
              setCapturing(isCapturing ? null : action.id)
            }}
          >
            {isCapturing ? 'Press a shortcut…' : <kbd>{formatShortcut(keymap[action.id] ?? null)}</kbd>}
          </button>
          {rowError && (
            <span className="shortcut-error" role="alert">
              {rowError}
            </span>
          )}
        </td>
        <td className="shortcut-actions">
          {keymap[action.id] !== action.default && (
            <Button size="sm" variant="ghost" onClick={() => void save({ ...keymap, [action.id]: action.default })}>
              Reset
            </Button>
          )}
          {keymap[action.id] !== null && (
            <Button size="sm" variant="ghost" onClick={() => void save({ ...keymap, [action.id]: null })}>
              Clear
            </Button>
          )}
        </td>
      </tr>
    )
  }

  const editorActions = ACTIONS.filter((action) => action.scope !== 'recorder')
  const recorderActions = ACTIONS.filter((action) => action.scope === 'recorder')
  return (
    <div className="keybindings">
      <table className="keybinding-table">
        <caption className="visually-hidden">Editor shortcuts</caption>
        <tbody>{editorActions.map(row)}</tbody>
      </table>
      <h3 className="section-title">Recorder</h3>
      <table className="keybinding-table">
        <caption className="visually-hidden">Recorder shortcuts</caption>
        <tbody>{recorderActions.map(row)}</tbody>
      </table>
      <div className="keybindings__footer">
        <p className="muted small">Ctrl+S always shows the save status and Ctrl+O opens the project list.</p>
        <Button busy={saving} onClick={() => void save(defaultKeymap())}>
          Restore defaults
        </Button>
      </div>
    </div>
  )
}
