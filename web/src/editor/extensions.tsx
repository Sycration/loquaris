// Extension points for the features other parts of the app add to the
// editor: audio (recorder, lesson playback), Latin analysis (search panel,
// first-use labels, highlights, macronize), export and version history.
//
// How to plug in: import `editorExtensions` and assign / push at module load,
// from a module that EditorPage imports (see the `import './plugins'` line in
// EditorPage.tsx — create web/src/editor/plugins.ts or edit it). Everything
// receives the EditorServices (doc, store, ui, presence, commands, …).
//
// Reacting to changes needs no hooks here: subscribe to
//   services.store.subscribeStructure / subscribeRecordings / subscribeEntry
//   services.ui.subscribe (selection, editing, active lesson)
//   services.presence.subscribe (peers; set your own `recording` flag with
//   services.presence.setRecording(entryId | null))
import type { ComponentType, ReactNode } from 'react'
import type { EntrySnapshot } from '../doc/projection'
import { LessonPlayButtonStub, type LessonPlayButtonProps } from '../audio/LessonPlayButton'
import type { MenuItem } from '../ui/Menu'
import { toast } from '../ui/toasts'
import type { EditorServices } from './context'

/** A range of an entry's text, in UTF-16 code units. */
export interface TextRange {
  start: number
  end: number
}

export interface SidePanel {
  id: string
  /** Rendered in the editor's right column, in registration order. With no
   * panels registered the script takes the full width. */
  render: (services: EditorServices) => ReactNode
}

export interface CardDecorationProps {
  services: EditorServices
  entry: EntrySnapshot
  /** Whether the pointer is over the card (hover buttons are visible). */
  hovered: boolean
}

export interface LessonFooterProps {
  services: EditorServices
  lessonId: string
  /** Position in script order, from 0. */
  lessonIndex: number
}

export interface EntryMenuContext {
  services: EditorServices
  /** The entry right-clicked. */
  entry: EntrySnapshot
  /** Every selected entry, in script order (includes `entry`). */
  selection: string[]
}

export interface EditorExtensions {
  /** Side panel sections (search, uses per lesson, new words…). */
  sidePanels: SidePanel[]
  /** Rendered in each card's top row, on the side away from the language
   * label (FEATURES §4: first-use lemmas, yellow, top-right for teachers,
   * top-left for students). */
  cardDecorations: ComponentType<CardDecorationProps>[]
  /** Rendered inside each lesson's footer bar, between its label and the
   * collapse chevron (spaced repetition: "Not reviewed in this lesson"). */
  lessonFooterDecorations: ComponentType<LessonFooterProps>[]
  /** Extra items appended to an entry's context menu (single or multi). */
  entryMenuItems: ((context: EntryMenuContext) => MenuItem[])[]
  /** Items added to the project menu in the editor header (Export…,
   * Version history…). */
  projectMenuItems: ((services: EditorServices) => MenuItem[])[]
  /** Components mounted once in the editor; dialogs that manage their own
   * open state (recorder, export, version history, parsing review). */
  editorMounts: ComponentType<{ services: EditorServices }>[]
  /** A React hook returning the ranges of an entry's text to highlight as
   * search matches (yellow, underlined). Must be a hook-safe function. */
  useEntryHighlights: (services: EditorServices, entryId: string) => readonly TextRange[]
  /** Called with the query when text is selected in a card that isn't being
   * edited (already filtered per FEATURES §4: trimmed, single line, ≤100). */
  onViewTextSelected: ((services: EditorServices, entryId: string, query: string) => void) | null
  /** Open the recorder for an entry (card speaker button). */
  openRecorder: (services: EditorServices, entryId: string) => void
  /** Context menu "Auto-macronize" on a single Latin entry. */
  autoMacronize: (services: EditorServices, entryId: string) => void
  /** The play / pause / resume button in each lesson header. */
  LessonPlayButton: ComponentType<LessonPlayButtonProps>
  /** True while this user is recording: undo/redo are refused ("Stop
   * recording before undo or redo."). */
  isRecording: (services: EditorServices) => boolean
}

const NO_HIGHLIGHTS: readonly TextRange[] = []

export const editorExtensions: EditorExtensions = {
  sidePanels: [],
  cardDecorations: [],
  lessonFooterDecorations: [],
  entryMenuItems: [],
  projectMenuItems: [],
  editorMounts: [],
  useEntryHighlights: () => NO_HIGHLIGHTS,
  onViewTextSelected: null,
  // TODO(audio agent): replace with the recorder dialog.
  openRecorder: () => toast.info('Recording isn’t available yet.'),
  // TODO(analysis agent): replace with macronize / parsing review.
  autoMacronize: () => toast.info('Auto-macronize isn’t available yet: the Latin analyzer is still loading.'),
  LessonPlayButton: LessonPlayButtonStub,
  isRecording: () => false,
}
