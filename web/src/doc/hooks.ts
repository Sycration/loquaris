// React bindings for DocStore. Each hook subscribes to one slice only.
import { useCallback, useSyncExternalStore } from 'react'
import type { DocStore, EntrySnapshot, LessonSnapshot, ScriptStructure } from './projection'
import type { Meta } from './schema'

export function useStructure(store: DocStore): ScriptStructure {
  return useSyncExternalStore(store.subscribeStructure, store.getStructure)
}

export function useMeta(store: DocStore): Meta {
  return useSyncExternalStore(store.subscribeMeta, store.getMeta)
}

export function useEntry(store: DocStore, id: string): EntrySnapshot | undefined {
  const subscribe = useCallback((listener: () => void) => store.subscribeEntry(id, listener), [store, id])
  return useSyncExternalStore(subscribe, () => store.getEntry(id))
}

export function useLesson(store: DocStore, id: string): LessonSnapshot | undefined {
  const subscribe = useCallback((listener: () => void) => store.subscribeLesson(id, listener), [store, id])
  return useSyncExternalStore(subscribe, () => store.getLesson(id))
}

/** "Recordings incomplete" or the lesson's total length; re-renders only
 * when the string changes. */
export function useLessonSummary(store: DocStore, lessonId: string): string {
  return useSyncExternalStore(store.subscribeRecordings, () => store.lessonSummary(lessonId))
}
