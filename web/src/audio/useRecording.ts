import { useSyncExternalStore } from 'react'
import { isRecordingNow, recordingChanges } from './session'

/** Whether this user is recording (re-renders on change). */
export function useIsRecording(): boolean {
  return useSyncExternalStore(recordingChanges.subscribe, isRecordingNow)
}
