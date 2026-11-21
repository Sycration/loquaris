// Plugs audio into the editor (see editor/extensions.tsx).
import { editorExtensions } from '../editor/extensions'
import { LessonPlayButton } from './LessonPlayButton'
import { openRecorderFor, RecorderMount } from './Recorder'
import { isRecordingNow } from './session'

editorExtensions.openRecorder = (services, entryId) => openRecorderFor(services, entryId)
editorExtensions.LessonPlayButton = LessonPlayButton
editorExtensions.isRecording = () => isRecordingNow()
editorExtensions.editorMounts.push(RecorderMount)
