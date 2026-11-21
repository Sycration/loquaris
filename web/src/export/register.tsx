import { editorExtensions } from '../editor/extensions'
import { ExportMount } from './ExportDialog'
import { setExportOpen } from './store'

editorExtensions.projectMenuItems.push(() => [{ label: 'Export…', onSelect: () => setExportOpen(true) }])
editorExtensions.editorMounts.push(ExportMount)
