import { useState, useSyncExternalStore } from 'react'
import type { EditorServices } from '../../editor/context'
import { Button } from '../../ui/Button'
import { Dialog } from '../../ui/Dialog'
import type { Review } from '../client.ts'
import { applyReviewChoices, reviewStoreFor, type PendingReview } from '../review'
import { sessionFor } from '../session'

/** Mounted once in the editor; opens when auto-macronize needs a decision. */
export function ParsingReviewMount({ services }: { services: EditorServices }) {
  const store = reviewStoreFor(sessionFor(services.store))
  const pending = useSyncExternalStore(store.subscribe, store.getSnapshot)
  if (!pending) return null
  return <ParsingReviewDialog key={pending.review.original + pending.entryId} services={services} pending={pending} onClose={() => store.set(null)} />
}

function previewSpelling(review: Review, index: number, choice: number | null): string {
  const word = review.words[index]
  return (choice !== null ? word.readings[choice]?.spelling : null) ?? word.text
}

export function ParsingReviewDialog({ services, pending, onClose }: { services: EditorServices; pending: PendingReview; onClose: () => void }) {
  const { review } = pending
  const [choices, setChoices] = useState<(number | null)[]>(() => review.words.map((word) => word.preselected))
  const [selected, setSelected] = useState(0)
  const [busy, setBusy] = useState(false)
  const word = review.words[selected]

  const choose = (value: number | null) => setChoices((current) => current.map((c, i) => (i === selected ? value : c)))
  const apply = async () => {
    setBusy(true)
    try {
      if (await applyReviewChoices(services, pending, choices)) onClose()
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title="Review Latin parsings"
      description="The reading selected from context is marked; other readings are not ranked."
      size="lg"
      className="parsing-review"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={apply} busy={busy}>
            Apply
          </Button>
        </>
      }
    >
      <div className="parsing-review__words" role="group" aria-label="Choose a word">
        <span className="parsing-review__label">Choose a word:</span>
        {review.words.map((w, i) => (
          <button
            key={`${w.start}`}
            type="button"
            className={['chip chip--latin', i === selected && 'chip--active'].filter(Boolean).join(' ')}
            aria-pressed={i === selected}
            onClick={() => setSelected(i)}
          >
            {previewSpelling(review, i, choices[i])}
          </button>
        ))}
      </div>
      {word && (
        <fieldset className="parsing-review__readings">
          <legend>
            Readings for <span className="latin">{word.text}</span>
          </legend>
          <label className="parsing-review__reading">
            <input type="radio" name="reading" checked={choices[selected] === null} onChange={() => choose(null)} />
            <span>Keep original spelling</span>
          </label>
          {word.readings.length === 0 && <p className="parsing-review__note">No dictionary analyses; the original spelling will be kept.</p>}
          {word.displayOrder.map((index) => {
            const reading = word.readings[index]
            if (!reading) return null
            const disabled = reading.spelling === null
            return (
              <label
                key={index}
                className={['parsing-review__reading', disabled && 'parsing-review__reading--disabled'].filter(Boolean).join(' ')}
                title={disabled ? 'This reading has a different spelling, so it cannot be applied safely.' : undefined}
              >
                <input type="radio" name="reading" disabled={disabled} checked={choices[selected] === index} onChange={() => choose(index)} />
                <span>
                  <span className="latin">{reading.spelling ?? word.text}</span> — {reading.lemma.headword}
                  {reading.morphology ? ` · ${reading.morphology}` : ''}
                  {index === word.contextReading ? ' · selected from context' : ''}
                </span>
              </label>
            )
          })}
        </fieldset>
      )}
    </Dialog>
  )
}
