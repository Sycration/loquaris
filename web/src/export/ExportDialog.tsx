// Export dialog (FEATURES §18): PDF, WAV or the desktop .loqr file.
import { useEffect, useState } from 'react'
import { urls } from '../api/client'
import { useMeta, useStructure } from '../doc/hooks'
import { setMeta } from '../doc/ops'
import type { Paper } from '../doc/schema'
import type { EditorServices } from '../editor/context'
import { Button } from '../ui/Button'
import { Dialog } from '../ui/Dialog'
import { Checkbox, SelectField } from '../ui/Field'
import { errorMessage, toast } from '../ui/toasts'
import { downloadFile } from './download'
import { FONT_SIZES, INCOMPLETE_MESSAGE, PAPER_SIZES, incompleteLessons, type LessonAudioInfo } from './helpers'
import { setExportOpen, useExportOpen } from './store'

type Format = 'pdf' | 'wav' | 'loqr'

export function ExportMount({ services }: { services: EditorServices }) {
  const open = useExportOpen()
  return open ? <ExportDialog services={services} /> : null
}

function ExportDialog({ services }: { services: EditorServices }) {
  const { store, doc, projectId, readOnly } = services
  const meta = useMeta(store)
  const structure = useStructure(store)
  const [format, setFormat] = useState<Format>('pdf')
  const [busy, setBusy] = useState(false)
  // PDF: null = all lessons; otherwise the chosen set.
  const [pdfLessons, setPdfLessons] = useState<Set<string> | null>(null)
  const [wavScope, setWavScope] = useState<string>('all')
  const [teacher, setTeacher] = useState(true)
  const [student, setStudent] = useState(true)

  // Recordings change without touching the structure: re-render on them.
  const [, setTick] = useState(0)
  useEffect(() => store.subscribeRecordings(() => setTick((t) => t + 1)), [store])

  const lessons: (LessonAudioInfo & { title: string })[] = structure.lessons.map((lesson, index) => ({
    id: lesson.id,
    name: store.getLesson(lesson.id)?.name ?? '',
    title: store.getLesson(lesson.id)?.name || `Lesson ${index + 1}`,
    recorded: lesson.entryIds.map((id) => !!store.getEntry(id)?.audio),
  }))

  const wavInScope = wavScope === 'all' ? lessons : lessons.filter((l) => l.id === wavScope)
  const incomplete = incompleteLessons(wavInScope)
  const wavBlocked = wavInScope.length === 0 || incomplete.length > 0
  const noAudioKind = !teacher && !student
  const pdfNone = pdfLessons !== null && pdfLessons.size === 0

  const canExport =
    !busy && (format === 'pdf' ? !pdfNone : format === 'wav' ? !wavBlocked && !noAudioKind : true)

  const run = async () => {
    setBusy(true)
    try {
      let url: string
      let fallback: string
      if (format === 'pdf') {
        const chosen = pdfLessons && pdfLessons.size < lessons.length ? lessons.filter((l) => pdfLessons.has(l.id)).map((l) => l.id) : undefined
        url = urls.exportPdf(projectId, chosen)
        fallback = 'script.pdf'
      } else if (format === 'wav') {
        url = urls.exportWav(projectId, { lessons: wavScope === 'all' ? undefined : [wavScope], teacher, student })
        fallback = 'lessons.wav'
      } else {
        url = urls.exportLoqr(projectId)
        fallback = 'project.loqr'
      }
      const name = await downloadFile(url, fallback)
      toast.success(`Exported ${name}.`)
    } catch (error) {
      toast.error(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const toggleLesson = (id: string, checked: boolean) => {
    const next = new Set(pdfLessons ?? lessons.map((l) => l.id))
    if (checked) next.add(id)
    else next.delete(id)
    setPdfLessons(next)
  }

  return (
    <Dialog
      open
      onClose={() => !busy && setExportOpen(false)}
      title="Export"
      allowHistory
      footer={
        <>
          <Button variant="ghost" onClick={() => setExportOpen(false)} disabled={busy}>
            Close
          </Button>
          <Button variant="primary" onClick={run} busy={busy} disabled={!canExport}>
            {busy ? 'Exporting…' : 'Export'}
          </Button>
        </>
      }
    >
      <SelectField label="Format" value={format} onChange={(e) => setFormat(e.target.value as Format)} disabled={busy}>
        <option value="pdf">PDF</option>
        <option value="wav">WAV audio</option>
        <option value="loqr">Desktop file (.loqr)</option>
      </SelectField>

      {format === 'pdf' && (
        <>
          <div className="export-row">
            <SelectField
              label="Paper"
              value={meta.paper}
              disabled={readOnly || busy}
              onChange={(e) => setMeta(doc, { paper: e.target.value as Paper })}
            >
              {PAPER_SIZES.map((p) => (
                <option key={p.value} value={p.value}>
                  {p.label}
                </option>
              ))}
            </SelectField>
            <SelectField
              label="Font size"
              value={meta.fontSize}
              disabled={readOnly || busy}
              onChange={(e) => setMeta(doc, { fontSize: Number(e.target.value) })}
            >
              {(FONT_SIZES.includes(meta.fontSize) ? FONT_SIZES : [...FONT_SIZES, meta.fontSize].sort((a, b) => a - b)).map((size) => (
                <option key={size} value={size}>
                  {size}
                </option>
              ))}
            </SelectField>
          </div>
          <fieldset className="export-lessons">
            <legend className="field__label">Lessons</legend>
            <Checkbox
              label="All lessons"
              checked={pdfLessons === null || pdfLessons.size === lessons.length}
              onChange={(e) => setPdfLessons(e.target.checked ? null : new Set())}
              disabled={busy}
            />
            {lessons.map((lesson) => (
              <Checkbox
                key={lesson.id}
                label={lesson.title}
                checked={pdfLessons === null || pdfLessons.has(lesson.id)}
                onChange={(e) => toggleLesson(lesson.id, e.target.checked)}
                disabled={busy}
              />
            ))}
            {pdfNone && <p className="field__error">Choose at least one lesson.</p>}
          </fieldset>
        </>
      )}

      {format === 'wav' && (
        <>
          <SelectField label="Scope" value={wavScope} onChange={(e) => setWavScope(e.target.value)} disabled={busy}>
            <option value="all">Entire project</option>
            {lessons.map((lesson) => (
              <option key={lesson.id} value={lesson.id}>
                {lesson.title}
              </option>
            ))}
          </SelectField>
          <fieldset className="export-lessons">
            <legend className="field__label">Include</legend>
            <Checkbox label="Teacher audio" checked={teacher} onChange={(e) => setTeacher(e.target.checked)} disabled={busy} />
            <Checkbox label="Student audio" checked={student} onChange={(e) => setStudent(e.target.checked)} disabled={busy} />
            {noAudioKind && <p className="field__error">Include teacher or student audio.</p>}
          </fieldset>
          {wavBlocked && (
            <p className="field__error" role="alert">
              {INCOMPLETE_MESSAGE}
              {incomplete.length > 0 && ` Not complete: ${incomplete.map((l) => l.name || 'Untitled').join(', ')}.`}
            </p>
          )}
        </>
      )}

      {format === 'loqr' && <p className="export-note">A file the desktop app can open, with all recordings included.</p>}
    </Dialog>
  )
}
