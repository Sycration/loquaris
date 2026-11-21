// The script column: toolbar, column headings, scrolling lessons, add-entry
// bar and the entry context menu (FEATURES §2–3).
import { useCallback, useEffect, useRef, type MouseEvent } from 'react'
import { useConnectionState } from '../collab/connection'
import { useLesson, useStructure } from '../doc/hooks'
import { formatShortcut } from '../keybindings/keybindings'
import { Button, IconButton } from '../ui/Button'
import { Plus, Redo, Undo } from '../ui/icons'
import { Menu } from '../ui/Menu'
import { EmptyState } from '../ui/misc'
import { useEditor } from './context'
import { useL1Name } from './EntryCard'
import { entryMenuItems } from './entryMenu'
import { LessonSection, lessonDisplayName } from './LessonSection'
import { useUi } from './uiStore'
import { useUndoState } from './useUndoState'

export function Script() {
  const services = useEditor()
  const { store, ui, commands, readOnly, connection } = services
  const structure = useStructure(store)
  const scrollRef = useRef<HTMLDivElement>(null)
  const lessonElements = useRef(new Map<string, HTMLElement>())
  const registerElement = useCallback((id: string, element: HTMLElement | null) => {
    if (element) lessonElements.current.set(id, element)
    else lessonElements.current.delete(id)
  }, [])
  const scrollRequest = useUi(ui, (state) => state.scrollRequest)
  const contextMenu = useUi(ui, (state) => state.contextMenu)

  // Scroll requests (reveal, new lesson, moved entries).
  useEffect(() => {
    if (!scrollRequest) return
    const container = scrollRef.current
    const selector = scrollRequest.kind === 'entry' ? `[data-entry-id="${scrollRequest.id}"]` : `[data-lesson-id="${scrollRequest.id}"]`
    const element = container?.querySelector<HTMLElement>(selector)
    if (element) {
      element.scrollIntoView({
        block: scrollRequest.center ? 'center' : scrollRequest.kind === 'lesson' ? 'start' : 'nearest',
        behavior: scrollRequest.center ? 'smooth' : 'auto',
      })
    }
  }, [scrollRequest])

  // Active lesson follows the lesson crossing the top of the viewport.
  const onScroll = useRef<number | null>(null)
  const handleScroll = () => {
    if (onScroll.current !== null) return
    onScroll.current = requestAnimationFrame(() => {
      onScroll.current = null
      const container = scrollRef.current
      if (!container) return
      const top = container.getBoundingClientRect().top + 1
      for (const [id, element] of lessonElements.current) {
        const rect = element.getBoundingClientRect()
        if (rect.top <= top && rect.bottom > top) {
          if (ui.getState().activeLesson !== id) commands.setActiveLesson(id)
          return
        }
      }
    })
  }

  const onDoubleClick = (event: MouseEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).closest('.card, .lesson-header, .lesson-footer, button, input, textarea, a')) return
    commands.deselect()
  }

  const { synced } = useConnectionState(connection)

  return (
    <div className="script">
      <div className="script__toolbar">
        {!readOnly && (
          <Button icon={<Plus />} onClick={() => commands.newLesson()}>
            New lesson
          </Button>
        )}
        {!readOnly && <HistoryButtons />}
      </div>
      <div className="script__headings" aria-hidden>
        <span className="script__heading">Teacher</span>
        <span className="script__heading script__heading--latin">Student</span>
      </div>
      <div className="script__scroll" ref={scrollRef} onScroll={handleScroll} onDoubleClick={onDoubleClick}>
        {structure.lessons.length === 0 ? (
          synced ? (
            <EmptyState
              title="This project has no lessons"
              action={
                !readOnly && (
                  <Button variant="primary" icon={<Plus />} onClick={() => commands.newLesson()}>
                    New lesson
                  </Button>
                )
              }
            />
          ) : null
        ) : (
          structure.lessons.map((lesson, index) => (
            <LessonSection key={lesson.id} lesson={lesson} index={index} lessonCount={structure.lessons.length} registerElement={registerElement} />
          ))
        )}
      </div>
      {!readOnly && structure.lessons.length > 0 && <AddEntryBar />}
      {contextMenu && (
        <Menu
          label="Entry actions"
          position={{ x: contextMenu.x, y: contextMenu.y }}
          items={entryMenuItems(services, contextMenu.entryId)}
          onClose={() => ui.setState({ contextMenu: null })}
        />
      )}
    </div>
  )
}

function HistoryButtons() {
  const { undo, commands, keymap } = useEditor()
  const { canUndo, canRedo } = useUndoState(undo)
  return (
    <>
      <IconButton label={`Undo (${formatShortcut(keymap.undo ?? null)})`} disabled={!canUndo} onClick={() => commands.undo()}>
        <Undo />
      </IconButton>
      <IconButton label={`Redo (${formatShortcut(keymap.redo ?? null)})`} disabled={!canRedo} onClick={() => commands.redo()}>
        <Redo />
      </IconButton>
    </>
  )
}

function AddEntryBar() {
  const { store, ui, commands } = useEditor()
  const structure = useStructure(store)
  const l1Name = useL1Name()
  // Re-render when the selection or active lesson changes the target.
  useUi(ui, (state) => `${state.selection.primary}|${state.activeLesson}`)
  const target = commands.targetLesson()
  const index = target ? (structure.lessonIndex.get(target) ?? 0) : 0
  const lesson = useLesson(store, target ?? '')
  return (
    <div className="add-bar">
      <p className="add-bar__target">
        Adding to lesson {index + 1}: {lessonDisplayName(lesson?.name ?? '')}
      </p>
      <div className="add-bar__buttons">
        <div className="add-bar__side">
          <button type="button" className="add-button add-button--l1" onClick={() => commands.add('teacher', 'l1')}>
            + {l1Name || 'L1'}
          </button>
          <button type="button" className="add-button add-button--latin" onClick={() => commands.add('teacher', 'latin')}>
            + Latin
          </button>
        </div>
        <div className="add-bar__side add-bar__side--student">
          <button type="button" className="add-button add-button--latin" onClick={() => commands.add('student', 'latin')}>
            + Latin
          </button>
        </div>
      </div>
    </div>
  )
}
