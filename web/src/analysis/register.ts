// Plugs the Latin analysis into the editor (imported by editor/plugins.ts).
import { createElement, useSyncExternalStore } from 'react'
import { editorExtensions } from '../editor/extensions'
import { autoMacronize } from './review'
import { sessionFor } from './session'
import { AnalysisHost } from './ui/AnalysisHost'
import { FirstUseLabels } from './ui/FirstUseLabels'
import { DueFlagMarker, DueForReviewSection, DueLessonFooter } from './ui/DueFlags'
import { ParsingReviewMount } from './ui/ParsingReviewDialog'
import { ContextsSection, NewWordsSection, SearchSection, UsesChartSection } from './ui/SearchPanel'
import './ui/analysis.css'

// Hot reloading re-runs this module in development, so replace what an
// earlier run registered rather than adding it twice.
const panels = [
  { id: 'analysis-search', render: (services) => createElement(SearchSection, { services }) },
  { id: 'analysis-new', render: (services) => createElement(NewWordsSection, { services }) },
  { id: 'analysis-due', render: (services) => createElement(DueForReviewSection, { services }) },
  { id: 'analysis-uses', render: (services) => createElement(UsesChartSection, { services }) },
  { id: 'analysis-contexts', render: (services) => createElement(ContextsSection, { services }) },
] satisfies typeof editorExtensions.sidePanels
const ids = new Set(panels.map((panel) => panel.id))
editorExtensions.sidePanels = [...editorExtensions.sidePanels.filter((panel) => !ids.has(panel.id)), ...panels]
const decorations = [FirstUseLabels, DueFlagMarker]
editorExtensions.cardDecorations = [
  ...editorExtensions.cardDecorations.filter((component) => !decorations.some((own) => own.name === component.name)),
  ...decorations,
]
editorExtensions.lessonFooterDecorations = [
  ...editorExtensions.lessonFooterDecorations.filter((component) => component.name !== DueLessonFooter.name),
  DueLessonFooter,
]
const mounts = [AnalysisHost, ParsingReviewMount]
const mountNames = new Set(mounts.map((component) => component.name))
editorExtensions.editorMounts = [...editorExtensions.editorMounts.filter((component) => !mountNames.has(component.name)), ...mounts]

editorExtensions.useEntryHighlights = (services, entryId) => {
  const { search } = sessionFor(services.store)
  return useSyncExternalStore(search.subscribe, () => search.rangesFor(entryId))
}
editorExtensions.onViewTextSelected = (services, _entryId, query) => {
  const { search } = sessionFor(services.store)
  search.setQuery(query)
}
editorExtensions.autoMacronize = (services, entryId) => void autoMacronize(services, entryId)
