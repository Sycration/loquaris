// Binds a <textarea> to a Y.Text (DESIGN.md: text is edited through Y.Text so
// concurrent typing merges character by character).
//
// - Local input is diffed against the last synced value and applied as one
//   minimal change in a transaction with the local origin (so it is undoable
//   and grouped by the undo manager's capture timeout).
// - Remote changes (and undo/redo) rewrite the textarea's value and move the
//   local selection through the change's delta, so the caret stays put
//   relative to the surrounding text.
// - IME compositions are left alone: the browser owns the value until
//   compositionend; remote deltas arriving meanwhile are queued and the
//   composed change is transformed through them before it is applied.
import * as Y from 'yjs'
import { LOCAL_ORIGIN } from '../doc/schema'
import { diffText, transformChange, transformSelection, type DeltaOp, type TextSelection } from './textDiff'

export interface TextBindingOptions {
  /** Called whenever the textarea's value changes, from either side. */
  onValue?: (value: string) => void
  /** Called with the local selection whenever it may have changed while the
   * textarea has focus. */
  onSelection?: (selection: TextSelection) => void
  /** The browser's own undo (context menu, Edit menu) is redirected here. */
  onHistory?: (kind: 'undo' | 'redo') => void
  /** Transform pasted/typed text before it is applied (e.g. strip newlines). */
  sanitize?: (value: string) => string
}

export class TextareaBinding {
  readonly element: HTMLTextAreaElement
  readonly ytext: Y.Text
  private options: TextBindingOptions
  private base: string
  private composing = false
  private applying = false
  private pending: DeltaOp[][] = []
  private disposers: (() => void)[] = []

  constructor(element: HTMLTextAreaElement, ytext: Y.Text, options: TextBindingOptions = {}) {
    this.element = element
    this.ytext = ytext
    this.options = options
    this.base = ytext.toString()
    element.value = this.base

    const observer = (event: Y.YTextEvent) => {
      if (this.applying) return
      if (this.composing) {
        this.pending.push(event.delta as DeltaOp[])
        return
      }
      this.refresh([event.delta as DeltaOp[]])
    }
    ytext.observe(observer)
    this.disposers.push(() => ytext.unobserve(observer))

    const listen = <K extends keyof HTMLElementEventMap>(type: K, handler: (event: HTMLElementEventMap[K]) => void) => {
      element.addEventListener(type, handler)
      this.disposers.push(() => element.removeEventListener(type, handler))
    }
    listen('input', (event) => {
      if (this.composing || (event as InputEvent).isComposing) {
        this.options.onValue?.(element.value)
        return
      }
      this.sync()
    })
    listen('compositionstart', () => {
      this.composing = true
    })
    listen('compositionend', () => {
      this.composing = false
      this.sync()
    })
    listen('beforeinput', (event) => {
      const type = (event as InputEvent).inputType
      if ((type === 'historyUndo' || type === 'historyRedo') && this.options.onHistory) {
        event.preventDefault()
        this.options.onHistory(type === 'historyUndo' ? 'undo' : 'redo')
      }
    })
    const reportSelection = () => {
      if (element.ownerDocument.activeElement === element) this.options.onSelection?.(this.selection())
    }
    listen('focus', reportSelection)
    listen('select', reportSelection)
    listen('keyup', reportSelection)
    listen('mouseup', reportSelection)
    const onSelectionChange = () => reportSelection()
    element.ownerDocument.addEventListener('selectionchange', onSelectionChange)
    this.disposers.push(() => element.ownerDocument.removeEventListener('selectionchange', onSelectionChange))
  }

  setOptions(options: TextBindingOptions): void {
    this.options = options
  }

  destroy(): void {
    for (const dispose of this.disposers) dispose()
    this.disposers = []
  }

  selection(): TextSelection {
    const { selectionStart, selectionEnd, selectionDirection } = this.element
    return { start: selectionStart, end: selectionEnd, direction: selectionDirection ?? 'none' }
  }

  /** Apply the textarea's current value to the Y.Text. */
  sync(): void {
    const element = this.element
    let value = element.value
    if (this.options.sanitize) {
      const clean = this.options.sanitize(value)
      if (clean !== value) {
        const caret = Math.max(0, element.selectionEnd - (value.length - clean.length))
        element.value = clean
        element.setSelectionRange(caret, caret)
        value = clean
      }
    }
    let change = diffText(this.base, value, element.selectionEnd)
    const pending = this.pending
    this.pending = []
    if (!change) {
      if (pending.length > 0) this.refresh(pending)
      return
    }
    for (const delta of pending) change = transformChange(change, delta)
    const doc = this.ytext.doc
    const apply = () => {
      if (change!.deleteCount > 0) this.ytext.delete(change!.index, change!.deleteCount)
      if (change!.insert) this.ytext.insert(change!.index, change!.insert)
    }
    this.applying = true
    try {
      if (doc) doc.transact(apply, LOCAL_ORIGIN)
      else apply()
    } finally {
      this.applying = false
    }
    if (pending.length > 0) {
      // The textarea lacks the remote changes made during the composition.
      element.value = this.ytext.toString()
      const caret = change.index + change.insert.length
      element.setSelectionRange(caret, caret)
    }
    this.base = element.value
    this.options.onValue?.(this.base)
    if (element.ownerDocument.activeElement === element) this.options.onSelection?.(this.selection())
  }

  /** Show the Y.Text's value, moving the selection through `deltas`. */
  private refresh(deltas: DeltaOp[][]): void {
    const element = this.element
    const focused = element.ownerDocument.activeElement === element
    let selection = this.selection()
    for (const delta of deltas) selection = transformSelection(selection, delta)
    const value = this.ytext.toString()
    if (element.value !== value) element.value = value
    this.base = value
    if (focused) element.setSelectionRange(selection.start, selection.end, selection.direction)
    this.options.onValue?.(value)
  }
}
