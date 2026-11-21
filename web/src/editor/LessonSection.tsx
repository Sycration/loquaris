// A lesson: sticky header (collapse, name, play, recording summary, lesson
// menu), its entries in two columns, and a footer bar (FEATURES §3).
import { memo, useCallback, useMemo } from 'react'
import * as Y from 'yjs'
import { useLesson, useLessonSummary } from '../doc/hooks'
import type { StructureLesson } from '../doc/ops'
import { roots } from '../doc/schema'
import { ChevronDown, ChevronRight, ChevronUp, More } from '../ui/icons'
import { MenuButton, type MenuItem } from '../ui/Menu'
import { useEditor } from './context'
import { DecoratedText } from './DecoratedText'
import { EntryCard } from './EntryCard'
import { editorExtensions } from './extensions'
import { TextEditor, useRemoteMarks } from './TextEditor'
import { useUi } from './uiStore'

export function lessonDisplayName(name: string): string {
  return name.trim() || 'Untitled'
}

interface LessonSectionProps {
  lesson: StructureLesson
  index: number
  lessonCount: number
  registerElement: (id: string, element: HTMLElement | null) => void
}

export const LessonSection = memo(function LessonSection({ lesson, index, lessonCount, registerElement }: LessonSectionProps) {
  const { ui, commands } = useEditor()
  const collapsed = useUi(ui, useCallback((state) => state.collapsed.has(lesson.id), [lesson.id]))
  const bodyId = `lesson-body-${lesson.id}`
  return (
    <section
      className={['lesson', collapsed && 'lesson--collapsed'].filter(Boolean).join(' ')}
      data-lesson-id={lesson.id}
      ref={(element) => registerElement(lesson.id, element)}
      aria-label={`Lesson ${index + 1}`}
    >
      <LessonHeader lesson={lesson} index={index} lessonCount={lessonCount} collapsed={collapsed} bodyId={bodyId} />
      {!collapsed && (
        <>
          <div
            className="lesson__body"
            id={bodyId}
            role="list"
            onClick={(event) => {
              if (event.target === event.currentTarget) commands.setActiveLesson(lesson.id)
            }}
          >
            {lesson.entryIds.length === 0 ? (
              <p className="lesson__empty" role="listitem">
                No entries yet. Add one with the buttons below.
              </p>
            ) : (
              lesson.entryIds.map((id) => <EntryRow key={id} id={id} />)
            )}
          </div>
          <LessonFooter lessonId={lesson.id} index={index} />
        </>
      )}
    </section>
  )
})

const EntryRow = memo(function EntryRow({ id }: { id: string }) {
  return (
    <div className="script-row" role="listitem">
      <EntryCard id={id} />
    </div>
  )
})

function LessonHeader({ lesson, index, lessonCount, collapsed, bodyId }: { lesson: StructureLesson; index: number; lessonCount: number; collapsed: boolean; bodyId: string }) {
  const services = useEditor()
  const { store, commands, readOnly, presence, doc, ui } = services
  const snapshot = useLesson(store, lesson.id)
  const summary = useLessonSummary(store, lesson.id)
  const focusToken = useUi(ui, useCallback((state) => (state.focusRequest?.kind === 'lesson' && state.focusRequest.id === lesson.id ? state.focusRequest.nonce : undefined), [lesson.id]))
  const name = snapshot?.name ?? ''
  const ytext = useMemo(() => {
    const value = roots(doc).lessons.get(lesson.id)?.get('name')
    return value instanceof Y.Text ? value : null
    // Re-read when the lesson snapshot changes, in case the name was created.
  }, [doc, lesson.id, snapshot]) // eslint-disable-line react-hooks/exhaustive-deps
  const target = useMemo(() => ({ kind: 'lesson' as const, id: lesson.id }), [lesson.id])
  const marks = useRemoteMarks(presence, target, ytext, name)
  const Play = editorExtensions.LessonPlayButton

  const menu = (): MenuItem[] => [
    { label: 'Move lesson up', disabled: index === 0, onSelect: () => commands.moveLesson(lesson.id, -1) },
    { label: 'Move lesson down', disabled: index === lessonCount - 1, onSelect: () => commands.moveLesson(lesson.id, 1) },
    { label: 'Duplicate lesson', onSelect: () => commands.duplicateLesson(lesson.id) },
    { kind: 'separator' },
    {
      label: 'Delete lesson…',
      danger: true,
      disabled: lessonCount <= 1,
      title: lessonCount <= 1 ? 'A project needs at least one lesson' : undefined,
      onSelect: () => void commands.deleteLesson(lesson.id),
    },
  ]

  return (
    <header
      className="lesson-header"
      onClick={(event) => {
        if ((event.target as HTMLElement).closest('button, [role="menu"]')) return
        commands.focusLesson(lesson.id)
      }}
    >
      <button
        type="button"
        className="icon-btn icon-btn--ghost icon-btn--sm lesson-header__toggle"
        aria-expanded={!collapsed}
        aria-controls={collapsed ? undefined : bodyId}
        aria-label={collapsed ? 'Expand lesson' : 'Collapse lesson'}
        title={collapsed ? 'Expand lesson' : 'Collapse lesson'}
        onClick={() => commands.toggleCollapsed(lesson.id)}
      >
        {collapsed ? <ChevronRight /> : <ChevronDown />}
      </button>
      <div className="lesson-header__name">
        <span className="lesson-header__number">Lesson {index + 1}:</span>
        {!readOnly && ytext ? (
          <TextEditor
            ytext={ytext}
            presence={presence}
            target={target}
            singleLine
            placeholder="Lesson name"
            focusToken={focusToken}
            className="lesson-header__input"
            ariaLabel={`Lesson ${index + 1} name`}
            onFocus={() => commands.focusLesson(lesson.id)}
            onHistory={(kind) => (kind === 'undo' ? commands.undo() : commands.redo())}
          />
        ) : (
          <span className="lesson-header__text">
            <DecoratedText text={name} remote={marks} placeholder="Untitled" />
          </span>
        )}
      </div>
      <Play services={services} lessonId={lesson.id} lessonIndex={index} entryCount={lesson.entryIds.length} />
      <span className={['lesson-header__summary', summary === 'Recordings incomplete' && 'lesson-header__summary--incomplete'].filter(Boolean).join(' ')}>
        {summary}
      </span>
      {!readOnly && (
        <MenuButton label={`Lesson ${index + 1} actions`} items={menu} size="sm">
          <More />
        </MenuButton>
      )}
    </header>
  )
}

function LessonFooter({ lessonId, index }: { lessonId: string; index: number }) {
  const services = useEditor()
  const { store, commands } = services
  const snapshot = useLesson(store, lessonId)
  return (
    <button
      type="button"
      className="lesson-footer"
      onClick={() => commands.toggleCollapsed(lessonId)}
      aria-label={`Collapse lesson ${index + 1}`}
    >
      <span className="lesson-footer__label">
        Lesson {index + 1}: {lessonDisplayName(snapshot?.name ?? '')}
      </span>
      {editorExtensions.lessonFooterDecorations.map((Decoration, i) => (
        <Decoration key={i} services={services} lessonId={lessonId} lessonIndex={index} />
      ))}
      <ChevronUp />
    </button>
  )
}
