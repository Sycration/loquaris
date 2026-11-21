// One open project: the Y.Doc and everything attached to it. Created when the
// editor mounts and destroyed when it unmounts or the project changes.
import { useEffect, useRef, useState } from 'react'
import * as Y from 'yjs'
import type { User } from '../api/types'
import { Connection } from '../collab/connection'
import { colorForUser, PresenceStore } from '../collab/presence'
import { removeOrphans } from '../doc/ops'
import { DocStore } from '../doc/projection'
import { createUndoManager } from '../doc/undo'
import { EditorCommands } from './commands'
import { editorExtensions } from './extensions'
import type { EditorServices } from './context'
import { pruneSelection } from './selection'
import { UiStore } from './uiStore'

export interface ProjectSession {
  doc: Y.Doc
  connection: Connection
  store: DocStore
  presence: PresenceStore
  undo: Y.UndoManager
  ui: UiStore
  commands: EditorCommands
}

export interface SessionOptions {
  readOnly: boolean
  /** Lets the audio agent's isRecording see the services. */
  services: () => EditorServices | null
}

export function createSession(projectId: string, user: User, options: { isReadOnly: () => boolean; isRecording: () => boolean }): ProjectSession & { destroy: () => void } {
  const doc = new Y.Doc()
  const connection = new Connection(projectId, doc)
  const store = new DocStore(doc)
  const presence = new PresenceStore(connection.provider.awareness, {
    id: user.id,
    name: user.displayName,
    color: colorForUser(user.id),
  })
  const undo = createUndoManager(doc)
  const ui = new UiStore(`loquaris:collapsed:${user.id}:${projectId}`)
  const commands = new EditorCommands({ doc, store, ui, undo, connection, isReadOnly: options.isReadOnly, isRecording: options.isRecording })

  // Entries of concurrently deleted lessons: removed by whoever notices, once
  // we know we hold the server's state and may write.
  store.onOrphans = () => {
    if (!options.isReadOnly() && connection.getState().synced) removeOrphans(doc)
  }
  const unsubscribeSync = connection.subscribe(() => {
    if (connection.getState().synced && !options.isReadOnly() && store.getStructure().orphans.length > 0) removeOrphans(doc)
  })

  // Forget selected / edited entries that disappeared (remote delete, undo).
  const unsubscribeStructure = store.subscribeStructure(() => {
    const state = ui.getState()
    const exists = (id: string) => store.getEntry(id) !== undefined && store.getStructure().position.has(id)
    const selection = pruneSelection(state.selection, exists)
    const editing = state.editing !== null && !exists(state.editing) ? null : state.editing
    const activeLesson =
      state.activeLesson !== null && !store.getStructure().lessonIndex.has(state.activeLesson) ? null : state.activeLesson
    ui.setState({ selection, editing, activeLesson })
  })

  // Tell collaborators which entry we're on.
  const unsubscribeUi = ui.subscribe(() => {
    const { editing, selection } = ui.getState()
    presence.setEntry(editing ?? selection.primary)
  })

  return {
    doc,
    connection,
    store,
    presence,
    undo,
    ui,
    commands,
    destroy() {
      unsubscribeSync()
      unsubscribeStructure()
      unsubscribeUi()
      presence.destroy()
      undo.destroy()
      store.destroy()
      connection.destroy()
      doc.destroy()
    },
  }
}

/** The session for a project, recreated when the project or user changes. */
export function useProjectSession(projectId: string, user: User | null, readOnly: boolean, services: () => EditorServices | null): ProjectSession | null {
  const [session, setSession] = useState<ProjectSession | null>(null)
  const readOnlyRef = useRef(readOnly)
  useEffect(() => {
    readOnlyRef.current = readOnly
  }, [readOnly])
  const servicesRef = useRef(services)
  useEffect(() => {
    servicesRef.current = services
  })
  const userId = user?.id
  const userName = user?.displayName
  useEffect(() => {
    if (!user) return
    const created = createSession(projectId, user, {
      isReadOnly: () => readOnlyRef.current,
      isRecording: () => {
        const current = servicesRef.current()
        return current ? editorExtensions.isRecording(current) : false
      },
    })
    setSession(created)
    return () => {
      created.destroy()
      setSession(null)
    }
    // Recreate only for another project or user.
  }, [projectId, userId]) // eslint-disable-line react-hooks/exhaustive-deps

  // Keep the presence name current if the user renames themselves.
  useEffect(() => {
    if (!session || !user) return
    const state = session.presence.awareness.getLocalState()
    if (state) session.presence.awareness.setLocalStateField('user', { id: user.id, name: user.displayName, color: colorForUser(user.id) })
  }, [session, user, userName])
  return session
}
