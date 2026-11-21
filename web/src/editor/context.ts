// Everything an editor component needs, provided once by EditorPage.
import { createContext, useContext } from 'react'
import type * as Y from 'yjs'
import type { ProjectDetail, User } from '../api/types'
import type { Connection } from '../collab/connection'
import type { PresenceStore } from '../collab/presence'
import type { DocStore } from '../doc/projection'
import type { Keymap } from '../keybindings/keybindings'
import type { EditorCommands } from './commands'
import type { UiStore } from './uiStore'

export interface EditorServices {
  projectId: string
  project: ProjectDetail
  user: User
  doc: Y.Doc
  store: DocStore
  ui: UiStore
  presence: PresenceStore
  connection: Connection
  undo: Y.UndoManager
  /** Viewers (and anyone after losing access) can't change the document. */
  readOnly: boolean
  commands: EditorCommands
  keymap: Keymap
  openProjectDetails: (focus?: 'name') => void
  openShare: () => void
}

export const EditorContext = createContext<EditorServices | null>(null)

export function useEditor(): EditorServices {
  const services = useContext(EditorContext)
  if (!services) throw new Error('useEditor outside EditorContext')
  return services
}
