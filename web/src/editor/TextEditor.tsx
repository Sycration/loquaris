// An auto-growing <textarea> bound to a Y.Text, with collaborators' carets
// and selections drawn in a mirror behind it. Used for entry text and lesson
// names.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type FocusEvent } from 'react'
import type * as Y from 'yjs'
import { resolveSelection, usePeersInText, type PresenceStore, type TextTarget } from '../collab/presence'
import { DecoratedText, type RemoteMark } from './DecoratedText'
import { TextareaBinding } from './textBinding'

/** Carets and selections of collaborators inside `ytext`. Recomputed when
 * they move or the text changes (pass the current text as `text`). */
export function useRemoteMarks(presence: PresenceStore, target: TextTarget, ytext: Y.Text | null, text: string): RemoteMark[] {
  const peers = usePeersInText(presence, target)
  return useMemo(() => {
    if (!ytext || peers.length === 0) return []
    const marks: RemoteMark[] = []
    for (const peer of peers) {
      if (!peer.selection) continue
      const resolved = resolveSelection(peer.selection, ytext)
      if (!resolved) continue
      marks.push({
        clientId: peer.clientId,
        name: peer.user.name,
        color: peer.user.color,
        start: Math.min(resolved.anchor, resolved.head),
        end: Math.max(resolved.anchor, resolved.head),
        head: resolved.head,
      })
    }
    return marks
    // `text` invalidates positions after edits.
  }, [peers, ytext, text]) // eslint-disable-line react-hooks/exhaustive-deps
}

export interface TextEditorProps {
  ytext: Y.Text
  presence: PresenceStore
  target: TextTarget
  className?: string
  placeholder?: string
  /** No line breaks: Enter blurs, pasted newlines become spaces. */
  singleLine?: boolean
  autoFocus?: boolean
  /** Changes to this value re-focus the editor (focus requests). */
  focusToken?: number
  ariaLabel: string
  onKeyDown?: (event: KeyboardEvent<HTMLTextAreaElement>) => void
  onFocus?: (event: FocusEvent<HTMLTextAreaElement>) => void
  onBlur?: (event: FocusEvent<HTMLTextAreaElement>) => void
  onHistory?: (kind: 'undo' | 'redo') => void
}

export function TextEditor({
  ytext,
  presence,
  target,
  className,
  placeholder,
  singleLine,
  autoFocus,
  focusToken,
  ariaLabel,
  onKeyDown,
  onFocus,
  onBlur,
  onHistory,
}: TextEditorProps) {
  const ref = useRef<HTMLTextAreaElement>(null)
  const [value, setValue] = useState(() => ytext.toString())
  const bindingRef = useRef<TextareaBinding | null>(null)
  const optionsRef = useRef({ onHistory, presence, target, singleLine })
  useLayoutEffect(() => {
    optionsRef.current = { onHistory, presence, target, singleLine }
  })

  useLayoutEffect(() => {
    const element = ref.current!
    const binding = new TextareaBinding(element, ytext, {
      onValue: setValue,
      onSelection: (selection) => {
        const { presence, target } = optionsRef.current
        presence.setSelection(target, ytext, selection.start, selection.end, selection.direction === 'backward')
      },
      onHistory: (kind) => optionsRef.current.onHistory?.(kind),
      sanitize: (text) => (optionsRef.current.singleLine ? text.replace(/\r?\n/g, ' ') : text),
    })
    bindingRef.current = binding
    setValue(element.value)
    return () => {
      binding.destroy()
      bindingRef.current = null
    }
  }, [ytext])

  useEffect(() => {
    if (!autoFocus && focusToken === undefined) return
    const element = ref.current
    if (!element) return
    if (document.activeElement !== element) {
      element.focus({ preventScroll: true })
      const end = element.value.length
      element.setSelectionRange(end, end)
    }
  }, [autoFocus, focusToken])

  const marks = useRemoteMarks(presence, target, ytext, value)

  return (
    <div className={['text-editor', singleLine && 'text-editor--single', className].filter(Boolean).join(' ')}>
      <div className="text-editor__mirror" aria-hidden>
        <DecoratedText text={value} remote={marks} />
      </div>
      <textarea
        ref={ref}
        className="text-editor__input"
        rows={1}
        spellCheck
        aria-label={ariaLabel}
        placeholder={placeholder}
        data-doc-field
        onKeyDown={(event) => {
          if (singleLine && event.key === 'Enter') {
            event.preventDefault()
            event.currentTarget.blur()
          }
          onKeyDown?.(event)
        }}
        onFocus={onFocus}
        onBlur={(event) => {
          presence.clearSelection(target)
          onBlur?.(event)
        }}
      />
    </div>
  )
}
