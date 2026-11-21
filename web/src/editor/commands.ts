// Editor commands: the glue between user intent (clicks, menus, shortcuts)
// and document operations, selection and UI state. Each document change is
// one undo step; non-text commands end text editing (FEATURES §4).
import type * as Y from 'yjs'
import type { Connection } from '../collab/connection'
import {
  addEntry,
  addLesson,
  canMove,
  changeKind,
  deleteEntries,
  deleteLesson,
  duplicateEntries,
  duplicateLesson,
  moveBlock,
  moveLesson,
  type KindChange,
} from '../doc/ops'
import type { DocStore } from '../doc/projection'
import type { Language, Participant } from '../doc/schema'
import { asUndoStep } from '../doc/undo'
import { confirm } from '../ui/Dialog'
import { toast } from '../ui/toasts'
import {
  adjacentEntry,
  clickSelection,
  EMPTY_SELECTION,
  selectedIds,
  selectMany,
  selectOnly,
  type ClickModifiers,
} from './selection'
import { nextNonce, type UiStore } from './uiStore'

export interface CommandDeps {
  doc: Y.Doc
  store: DocStore
  ui: UiStore
  undo: Y.UndoManager
  connection: Connection
  isReadOnly: () => boolean
  /** True while this user is recording (audio agent). */
  isRecording: () => boolean
}

export type EntryShortcut =
  | 'duplicate_entry'
  | 'switch_language'
  | 'move_to_student'
  | 'move_to_teacher'
  | 'move_entry_up'
  | 'move_entry_down'

export class EditorCommands {
  private deps: CommandDeps

  constructor(deps: CommandDeps) {
    this.deps = deps
  }

  private get ui() {
    return this.deps.ui
  }
  private get store() {
    return this.deps.store
  }

  private writable(): boolean {
    return !this.deps.isReadOnly()
  }

  private step<T>(fn: () => T): T {
    return asUndoStep(this.deps.undo, fn)
  }

  // ---- selection -----------------------------------------------------------

  /** Selected entry IDs in script order. */
  selection(): string[] {
    return selectedIds(this.ui.getState().selection, this.store.getStructure().flat)
  }

  primary(): string | null {
    const { primary } = this.ui.getState().selection
    return primary !== null && this.store.getEntry(primary) ? primary : null
  }

  click(entryId: string, modifiers: ClickModifiers): void {
    const state = this.ui.getState()
    const selection = clickSelection(state.selection, entryId, modifiers, this.store.getStructure().flat)
    this.ui.setState({ selection, editing: null, activeLesson: this.store.getEntry(entryId)?.lesson ?? state.activeLesson })
  }

  /** Right click: keep a selection containing the entry, else select it. */
  contextClick(entryId: string): void {
    const state = this.ui.getState()
    const selection = clickSelection(state.selection, entryId, { mod: false, alt: false, shift: false }, this.store.getStructure().flat, true)
    this.ui.setState({ selection, editing: null, activeLesson: this.store.getEntry(entryId)?.lesson ?? state.activeLesson })
  }

  deselect(): void {
    this.ui.setState({ selection: EMPTY_SELECTION, editing: null })
  }

  stopEditing(): void {
    if (this.ui.getState().editing === null) return
    this.ui.setState({ editing: null })
    const active = document.activeElement
    if (active instanceof HTMLElement && active.closest('[data-entry-id]')) active.blur()
  }

  /** Escape: stop editing if editing, otherwise deselect. */
  escape(): void {
    if (this.ui.getState().editing !== null) this.stopEditing()
    else this.deselect()
  }

  /** Select one entry, expand its lesson, and scroll it into view. */
  reveal(entryId: string, center = true): void {
    const entry = this.store.getEntry(entryId)
    if (!entry) return
    this.expand(entry.lesson)
    this.ui.setState({
      selection: selectOnly(entryId),
      editing: null,
      activeLesson: entry.lesson,
      scrollRequest: { kind: 'entry', id: entryId, center, nonce: nextNonce() },
    })
  }

  selectAdjacent(next: boolean): void {
    const state = this.ui.getState()
    const structure = this.store.getStructure()
    const active = structure.lessons.find((lesson) => lesson.id === this.activeLesson())
    const target = adjacentEntry(state.selection, structure.flat, active?.entryIds ?? [], next)
    if (target) this.reveal(target, false)
  }

  edit(entryId: string): void {
    if (!this.writable()) return
    const entry = this.store.getEntry(entryId)
    if (!entry) return
    this.expand(entry.lesson)
    this.ui.setState({
      selection: selectOnly(entryId),
      editing: entryId,
      activeLesson: entry.lesson,
      focusRequest: { kind: 'entry', id: entryId, nonce: nextNonce() },
      scrollRequest: { kind: 'entry', id: entryId, nonce: nextNonce() },
    })
  }

  editSelected(): void {
    const primary = this.primary()
    if (primary) this.edit(primary)
  }

  // ---- lessons -------------------------------------------------------------

  /** The lesson new entries go to: the selection's lesson, else the active
   * lesson, else the first. */
  targetLesson(): string | null {
    const primary = this.primary()
    if (primary) return this.store.getEntry(primary)!.lesson
    return this.activeLesson()
  }

  activeLesson(): string | null {
    const { activeLesson } = this.ui.getState()
    const lessons = this.store.getStructure().lessons
    if (activeLesson && lessons.some((lesson) => lesson.id === activeLesson)) return activeLesson
    return lessons[0]?.id ?? null
  }

  setActiveLesson(lessonId: string): void {
    this.ui.setState({ activeLesson: lessonId })
  }

  /** Header click: make the lesson active and clear the selection. */
  focusLesson(lessonId: string): void {
    this.ui.setState({ activeLesson: lessonId, selection: EMPTY_SELECTION, editing: null })
  }

  isCollapsed(lessonId: string): boolean {
    return this.ui.getState().collapsed.has(lessonId)
  }

  expand(lessonId: string): void {
    const { collapsed } = this.ui.getState()
    if (!collapsed.has(lessonId)) return
    const next = new Set(collapsed)
    next.delete(lessonId)
    this.ui.setState({ collapsed: next })
  }

  toggleCollapsed(lessonId: string): void {
    const { collapsed, editing } = this.ui.getState()
    const next = new Set(collapsed)
    if (next.has(lessonId)) next.delete(lessonId)
    else next.add(lessonId)
    const editingHere = editing !== null && this.store.getEntry(editing)?.lesson === lessonId
    this.ui.setState({ collapsed: next, activeLesson: lessonId, ...(editingHere ? { editing: null } : {}) })
  }

  newLesson(): void {
    if (!this.writable()) return
    const id = this.step(() => addLesson(this.deps.doc))
    this.ui.setState({
      activeLesson: id,
      selection: EMPTY_SELECTION,
      editing: null,
      scrollRequest: { kind: 'lesson', id, nonce: nextNonce() },
      focusRequest: { kind: 'lesson', id, nonce: nextNonce() },
    })
  }

  async deleteLesson(lessonId: string): Promise<void> {
    if (!this.writable()) return
    const structure = this.store.getStructure()
    if (structure.lessons.length <= 1) {
      toast.info('A project needs at least one lesson.')
      return
    }
    const index = structure.lessonIndex.get(lessonId) ?? 0
    const name = this.store.getLesson(lessonId)?.name.trim()
    const count = structure.lessons[index]?.entryIds.length ?? 0
    const ok = await confirm({
      title: `Delete lesson ${index + 1}${name ? `: ${name}` : ''}?`,
      message:
        count > 0
          ? `Its ${count} ${count === 1 ? 'entry' : 'entries'} and their recordings are deleted for everyone. You can undo this.`
          : 'You can undo this.',
      confirmLabel: 'Delete lesson',
      danger: true,
    })
    if (!ok) return
    this.stopEditing()
    this.step(() => deleteLesson(this.deps.doc, lessonId))
  }

  duplicateLesson(lessonId: string): void {
    if (!this.writable()) return
    const copy = this.step(() => duplicateLesson(this.deps.doc, lessonId))
    if (copy) this.ui.setState({ activeLesson: copy, scrollRequest: { kind: 'lesson', id: copy, nonce: nextNonce() } })
  }

  moveLesson(lessonId: string, direction: -1 | 1): void {
    if (!this.writable()) return
    if (this.step(() => moveLesson(this.deps.doc, lessonId, direction))) {
      this.ui.setState({ scrollRequest: { kind: 'lesson', id: lessonId, nonce: nextNonce() } })
    }
  }

  // ---- entries -------------------------------------------------------------

  /** Insert a blank entry after the primary selection (in its lesson), or at
   * the end of the active lesson; select and edit it. */
  add(participant: Participant, language: Language): void {
    if (!this.writable()) return
    const lesson = this.targetLesson()
    if (!lesson) {
      toast.info('Add a lesson first.')
      return
    }
    const after = this.primary()
    const id = this.step(() => addEntry(this.deps.doc, { lesson, after, participant, language }))
    if (id) this.edit(id)
  }

  duplicate(ids: string[]): void {
    if (!this.writable() || ids.length === 0) return
    this.stopEditing()
    const primary = this.primary()
    const copies = this.step(() => duplicateEntries(this.deps.doc, ids))
    if (copies.size === 0) return
    const ordered = this.store.getStructure().flat.filter((id) => Array.from(copies.values()).includes(id))
    this.ui.setState({ selection: selectMany(ordered, primary ? (copies.get(primary) ?? null) : null) })
    this.scrollToPrimary()
  }

  delete(ids: string[]): void {
    if (!this.writable() || ids.length === 0) return
    this.stopEditing()
    this.step(() => deleteEntries(this.deps.doc, ids))
    this.ui.setState({ selection: EMPTY_SELECTION })
  }

  changeKind(ids: string[], change: KindChange): void {
    if (!this.writable() || ids.length === 0) return
    this.stopEditing()
    this.step(() => changeKind(this.deps.doc, ids, change))
  }

  canMove(ids: string[], direction: -1 | 1): boolean {
    return ids.length > 0 && canMove(this.store.getStructure(), ids, direction)
  }

  move(ids: string[], direction: -1 | 1): void {
    if (!this.writable() || ids.length === 0) return
    this.stopEditing()
    if (this.step(() => moveBlock(this.deps.doc, ids, direction))) {
      for (const id of ids) {
        const entry = this.store.getEntry(id)
        if (entry) this.expand(entry.lesson)
      }
      this.scrollToPrimary()
    }
  }

  /** Run an entry shortcut on the selection (desktop
   * `selected_entry_shortcut`): bulk with several selected. */
  entryShortcut(action: EntryShortcut): void {
    const ids = this.selection()
    if (ids.length === 0) return
    if (ids.length > 1) {
      switch (action) {
        case 'duplicate_entry':
          return this.duplicate(ids)
        case 'switch_language':
          return this.changeKind(ids, 'toggleLanguage')
        case 'move_to_student':
          return this.changeKind(ids, 'toStudent')
        case 'move_to_teacher':
          return this.changeKind(ids, 'toTeacher')
        case 'move_entry_up':
          return this.move(ids, -1)
        case 'move_entry_down':
          return this.move(ids, 1)
      }
    }
    const primary = this.primary() ?? ids[0]
    const entry = this.store.getEntry(primary)
    if (!entry) return
    const teacher = entry.participant === 'teacher'
    switch (action) {
      case 'duplicate_entry':
        return this.duplicate([primary])
      case 'switch_language':
        if (teacher) this.changeKind([primary], 'toggleLanguage')
        return
      case 'move_to_student':
        if (teacher) this.changeKind([primary], 'toStudent')
        return
      case 'move_to_teacher':
        if (!teacher) this.changeKind([primary], 'toTeacher')
        return
      case 'move_entry_up':
        return this.move([primary], -1)
      case 'move_entry_down':
        return this.move([primary], 1)
    }
  }

  private scrollToPrimary(): void {
    const primary = this.primary()
    if (primary) this.ui.setState({ scrollRequest: { kind: 'entry', id: primary, nonce: nextNonce() } })
  }

  // ---- history -------------------------------------------------------------

  undo(): void {
    this.history('undo')
  }

  redo(): void {
    this.history('redo')
  }

  private history(kind: 'undo' | 'redo'): void {
    if (!this.writable()) return
    if (this.deps.isRecording()) {
      toast.info('Stop recording before undo or redo.')
      return
    }
    const { undo } = this.deps
    if (kind === 'undo') undo.undo()
    else undo.redo()
  }

  /** Mod+S: everything is saved continuously; say so. */
  save(): void {
    const { status } = this.deps.connection.getState()
    if (status === 'saved') toast.success('All changes saved')
    else if (status === 'offline') toast.info('Offline — your changes are kept and will be saved when you reconnect.')
    else if (status === 'closed') toast.error('Disconnected from the server; recent changes may not be saved.')
    else toast.info('Saving…')
  }
}
