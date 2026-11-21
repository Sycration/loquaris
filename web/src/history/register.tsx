import { editorExtensions } from '../editor/extensions'
import { HistoryMount } from './VersionHistoryDialog'
import { setHistoryOpen } from './store'

editorExtensions.projectMenuItems.push(() => [{ label: 'Version history…', onSelect: () => setHistoryOpen(true) }])
editorExtensions.editorMounts.push(HistoryMount)
