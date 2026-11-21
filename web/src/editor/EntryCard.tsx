// One script entry (FEATURES §4): language label, text (viewed or edited),
// audio marker, hover buttons, collaborators working in it.
import { memo, useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore, type CSSProperties, type MouseEvent } from 'react'
import * as Y from 'yjs'
import { usePeersOnEntry } from '../collab/presence'
import { useEntry } from '../doc/hooks'
import { ensureText, roots } from '../doc/schema'
import { isMac } from '../keybindings/keybindings'
import { Pencil, Speaker } from '../ui/icons'
import { useEditor } from './context'
import { DecoratedText } from './DecoratedText'
import { editorExtensions } from './extensions'
import { isSelected, selectionSearchQuery } from './selection'
import { TextEditor, useRemoteMarks } from './TextEditor'
import { useUi } from './uiStore'

export function useL1Name(): string {
  const { store } = useEditor()
  return useSyncExternalStore(store.subscribeMeta, () => store.getMeta().l1Name)
}

export const EntryCard = memo(function EntryCard({ id }: { id: string }) {
  const services = useEditor()
  const { store, ui, presence, commands, readOnly, doc } = services
  const entry = useEntry(store, id)
  const selected = useUi(ui, useCallback((state) => isSelected(state.selection, id), [id]))
  const editing = useUi(ui, useCallback((state) => state.editing === id, [id]))
  const focusToken = useUi(ui, useCallback((state) => (state.focusRequest?.kind === 'entry' && state.focusRequest.id === id ? state.focusRequest.nonce : undefined), [id]))
  const peers = usePeersOnEntry(presence, id)
  const l1Name = useL1Name()
  const [hovered, setHovered] = useState(false)
  const cardRef = useRef<HTMLDivElement>(null)
  const highlights = editorExtensions.useEntryHighlights(services, id)
  const ytext = useMemo(() => {
    const text = roots(doc).entries.get(id)?.get('text')
    return text instanceof Y.Text ? text : null
    // Re-read whenever the entry changes (cheap), in case its text was
    // created or replaced.
  }, [doc, id, entry]) // eslint-disable-line react-hooks/exhaustive-deps
  const target = useMemo(() => ({ kind: 'entry' as const, id }), [id])
  const marks = useRemoteMarks(presence, target, ytext, entry?.text ?? '')
  const wantsEditor = editing && !readOnly
  useEffect(() => {
    // Documents from older clients may lack the Y.Text; create it to edit.
    const map = roots(doc).entries.get(id)
    if (wantsEditor && !ytext && map) ensureText(doc, map, 'text')
  }, [wantsEditor, ytext, doc, id])

  if (!entry) return null
  const latin = entry.language === 'latin'
  const teacher = entry.participant === 'teacher'
  const label = latin ? 'Latin' : l1Name || 'L1'
  const isEditing = wantsEditor
  const editingText = isEditing ? ytext : null
  const recordingPeers = peers.filter((peer) => peer.recording === id)
  const outline = peers[0]?.user.color

  const onClick = (event: MouseEvent<HTMLDivElement>) => {
    const element = event.target as HTMLElement
    if (element.closest('.card__actions, .card__decorations button, a')) return
    // Clicks inside the text being edited only move the caret.
    if (isEditing && element.closest('.text-editor')) return
    // A text selection in a viewed card is a search, not a click.
    if (!isEditing && element.closest('.card__text')) {
      const selection = window.getSelection()
      if (selection && !selection.isCollapsed && cardRef.current?.contains(selection.anchorNode)) {
        const query = selectionSearchQuery(selection.toString())
        if (query && editorExtensions.onViewTextSelected) editorExtensions.onViewTextSelected(services, id, query)
        return
      }
    }
    commands.click(id, { mod: isMac ? event.metaKey : event.ctrlKey, alt: event.altKey, shift: event.shiftKey })
  }

  return (
    <div
      ref={cardRef}
      className={[
        'card',
        latin ? 'card--latin' : 'card--l1',
        teacher ? 'card--teacher' : 'card--student',
        selected && 'card--selected',
        isEditing && 'card--editing',
        peers.length > 0 && 'card--remote',
      ]
        .filter(Boolean)
        .join(' ')}
      style={outline ? ({ '--remote-color': outline } as CSSProperties) : undefined}
      data-entry-id={id}
      role="group"
      aria-label={`${teacher ? 'Teacher' : 'Student'}, ${label}${entry.audio ? ', has audio' : ''}${selected ? ', selected' : ''}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onMouseDown={(event) => {
        // Shift-click selects a range of entries, not text.
        if (event.shiftKey && !isEditing) event.preventDefault()
      }}
      onClick={onClick}
      onContextMenu={(event) => {
        if (readOnly) return
        event.preventDefault()
        commands.contextClick(id)
        ui.setState({ contextMenu: { entryId: id, x: event.clientX, y: event.clientY } })
      }}
      onBlur={(event) => {
        if (!isEditing) return
        const next = event.relatedTarget as Node | null
        if (next && cardRef.current?.contains(next)) return
        // Leaving the window keeps editing; moving focus elsewhere ends it.
        requestAnimationFrame(() => {
          if (document.hasFocus() && !cardRef.current?.contains(document.activeElement) && ui.getState().editing === id) {
            ui.setState({ editing: null })
          }
        })
      }}
    >
      <div className="card__top">
        <span className="card__label">{label}</span>
        <span className="card__decorations">
          {editorExtensions.cardDecorations.map((Decoration, i) => (
            <Decoration key={i} services={services} entry={entry} hovered={hovered} />
          ))}
        </span>
      </div>

      {editingText ? (
        <TextEditor
          ytext={editingText}
          presence={presence}
          target={target}
          className="card__text card__text--editing"
          placeholder="Enter text…"
          autoFocus
          focusToken={focusToken}
          ariaLabel={`${teacher ? 'Teacher' : 'Student'} ${label} text`}
          onHistory={(kind) => (kind === 'undo' ? commands.undo() : commands.redo())}
        />
      ) : (
        <div className="card__text" lang={latin ? 'la' : undefined}>
          <DecoratedText text={entry.text} remote={marks} highlights={highlights} placeholder="Enter text…" />
        </div>
      )}

      {!readOnly && (
        <div className="card__actions">
          <button
            type="button"
            className={['card__action', entry.audio && 'card__action--active'].filter(Boolean).join(' ')}
            aria-label={entry.audio ? 'Edit attached audio' : 'Record or import audio'}
            title={entry.audio ? 'Edit attached audio' : 'Record or import audio'}
            onClick={() => {
              commands.stopEditing()
              editorExtensions.openRecorder(services, id)
            }}
          >
            <Speaker />
          </button>
          <button
            type="button"
            className="card__action"
            aria-label="Edit entry"
            title="Edit entry"
            onClick={() => commands.edit(id)}
          >
            <Pencil />
          </button>
        </div>
      )}

      {entry.audio && <span className="card__audio-marker" aria-hidden title="Audio attached" />}

      {peers.length > 0 && (
        <div className="card__peers" aria-label={`Also here: ${peers.map((peer) => peer.user.name).join(', ')}`}>
          {peers.map((peer) => (
            <span key={peer.clientId} className="card__peer" style={{ '--peer-color': peer.user.color } as CSSProperties}>
              {peer.user.name}
            </span>
          ))}
        </div>
      )}
      {recordingPeers.length > 0 && (
        <span className="card__recording" role="status">
          ● Recording — {recordingPeers.map((peer) => peer.user.name).join(', ')}
        </span>
      )}
    </div>
  )
})
