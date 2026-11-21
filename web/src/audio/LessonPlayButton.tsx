// The play / pause / resume button in each lesson header (FEATURES §3, §9).
import { useSyncExternalStore } from 'react'
import type { EditorServices } from '../editor/context'
import { Pause, Play } from '../ui/icons'
import { lessonPlayState, togglePlayLesson } from './lessonPlayer'
import { useIsRecording } from './useRecording'

export interface LessonPlayButtonProps {
  services: EditorServices
  lessonId: string
  /** Zero-based position of the lesson in the script. */
  lessonIndex: number
  /** Number of entries in the lesson. */
  entryCount: number
}

export function LessonPlayButton({ services, lessonId, entryCount }: LessonPlayButtonProps) {
  const state = useSyncExternalStore(lessonPlayState.subscribe, lessonPlayState.get)
  const recording = useIsRecording()
  const mine = state.lessonId === lessonId ? state.status : 'idle'
  const disabled = recording || entryCount === 0
  const label = mine === 'playing' || mine === 'loading' ? 'Pause lesson' : mine === 'paused' ? 'Resume lesson' : 'Play lesson'
  const title = recording ? 'Stop recording first' : entryCount === 0 ? 'The lesson is empty' : label
  return (
    <button
      type="button"
      className="icon-btn icon-btn--ghost icon-btn--sm lesson-header__play"
      aria-label={label}
      title={title}
      disabled={disabled}
      onClick={() => togglePlayLesson(services, lessonId)}
    >
      {mine === 'playing' || mine === 'loading' ? <Pause /> : <Play />}
    </button>
  )
}

/** Name the editor's default extension points still import. */
export const LessonPlayButtonStub = LessonPlayButton
