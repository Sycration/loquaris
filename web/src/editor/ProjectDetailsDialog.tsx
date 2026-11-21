// Project details (FEATURES §12): name, learners' language, and the gaps
// between lines. Every change is a document change, so it is undoable and
// appears for collaborators immediately.
import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import { useMeta } from '../doc/hooks'
import { MAX_GAP_MS } from '../doc/schema'
import { setMeta } from '../doc/ops'
import { Button } from '../ui/Button'
import { Dialog } from '../ui/Dialog'
import { Checkbox, TextField } from '../ui/Field'
import { useEditor } from './context'

/** Seconds text → milliseconds, or an error message (0–60 s). */
export function parseGap(text: string): number | string {
  const value = Number(text.replace(',', '.'))
  if (text.trim() === '' || !Number.isFinite(value)) return 'Enter a number of seconds.'
  if (value < 0 || value > MAX_GAP_MS / 1000) return 'Use a value from 0 to 60 seconds.'
  return Math.round(value * 1000)
}

function GapField({ label, valueMs, onCommit, disabled, hint }: { label: string; valueMs: number; onCommit: (ms: number) => void; disabled: boolean; hint: string }) {
  const [text, setText] = useState((valueMs / 1000).toFixed(3))
  const [error, setError] = useState<string | null>(null)
  const focused = useRef(false)
  useEffect(() => {
    if (!focused.current) setText((valueMs / 1000).toFixed(3))
  }, [valueMs])
  return (
    <TextField
      label={label}
      type="number"
      inputMode="decimal"
      min={0}
      max={60}
      step={0.01}
      value={text}
      disabled={disabled}
      error={error}
      hint={hint}
      data-doc-field
      onFocus={() => (focused.current = true)}
      onBlur={() => {
        focused.current = false
        if (!error) setText((valueMs / 1000).toFixed(3))
      }}
      onChange={(event) => {
        setText(event.target.value)
        const parsed = parseGap(event.target.value)
        if (typeof parsed === 'string') setError(parsed)
        else {
          setError(null)
          onCommit(parsed)
        }
      }}
    />
  )
}

export function ProjectDetailsDialog({ open, onClose, focusName }: { open: boolean; onClose: () => void; focusName: boolean }) {
  const { store, doc, readOnly, project, undo } = useEditor()
  const meta = useMeta(store)
  const nameRef = useRef<HTMLInputElement>(null)
  const nameAtOpen = useRef(meta.name)
  useEffect(() => {
    if (open) nameAtOpen.current = meta.name
    // Only when opening.
  }, [open]) // eslint-disable-line react-hooks/exhaustive-deps

  const close = () => {
    // The project list shows the server's copy of the name; owners update it.
    if (project.role === 'owner' && meta.name !== nameAtOpen.current) {
      void api.projects.update(project.id, { name: meta.name }).catch(() => undefined)
    }
    undo.stopCapturing()
    onClose()
  }

  return (
    <Dialog
      open={open}
      onClose={close}
      title="Project details"
      allowHistory
      initialFocus={focusName ? nameRef : undefined}
      footer={
        <Button variant="primary" onClick={close}>
          Done
        </Button>
      }
    >
      <div className="form">
        <TextField
          ref={nameRef}
          label="Project name"
          value={meta.name}
          disabled={readOnly}
          placeholder="Untitled project"
          data-doc-field
          onChange={(event) => setMeta(doc, { name: event.target.value })}
        />
        <TextField
          label="Learners’ language (L1)"
          value={meta.l1Name}
          disabled={readOnly}
          data-doc-field
          hint="Labels teacher lines that aren’t Latin, and picks the language of dictionary translations."
          onChange={(event) => setMeta(doc, { l1Name: event.target.value })}
        />
        <div className="form__row">
          <GapField
            label="Gap, same speaker (s)"
            valueMs={meta.gapSameMs}
            disabled={readOnly}
            hint="Silence between two lines of one speaker."
            onCommit={(ms) => setMeta(doc, { gapSameMs: ms })}
          />
          <GapField
            label="Gap, speaker changes (s)"
            valueMs={meta.gapDifferentMs}
            disabled={readOnly}
            hint="Silence when the other speaker starts."
            onCommit={(ms) => setMeta(doc, { gapDifferentMs: ms })}
          />
        </div>
        <Checkbox
          label="Spaced repetition flags"
          checked={meta.spacedRepetition}
          disabled={readOnly}
          data-doc-field
          onChange={(event) => setMeta(doc, { spacedRepetition: event.target.checked })}
        />
        <p className="field__hint">Flags Latin words a learner would be due to review but the script doesn’t bring back in time.</p>
      </div>
    </Dialog>
  )
}
