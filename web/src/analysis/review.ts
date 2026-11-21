// Auto-macronize and parsing review (FEATURES §16, §17): ask the worker for
// the review, apply it directly when unambiguous, else hand it to the dialog.
import * as Y from 'yjs'
import type { AppliedReview, Review } from './client.ts'
import { entryLemmas, entryText, LOCAL_ORIGIN, roots } from '../doc/schema'
import { asUndoStep } from '../doc/undo'
import { diffText } from '../editor/textDiff'
import type { EditorServices } from '../editor/context'
import { toast } from '../ui/toasts'
import { sessionFor, type AnalysisSession } from './session'

/**
 * Write a review's text and lemma choices as ONE undo step: the text as a
 * minimal diff, the lemma choices replacing the entry's lemmas map. Returns
 * false if the entry no longer exists.
 */
export function writeReview(doc: Y.Doc, undo: Y.UndoManager | null, entryId: string, applied: AppliedReview): boolean {
  const entry = roots(doc).entries.get(entryId)
  if (!entry) return false
  asUndoStep(undo, () =>
    doc.transact(() => {
      const text = entryText(entry)
      if (text) {
        const change = diffText(text.toString(), applied.text)
        if (change) {
          if (change.deleteCount > 0) text.delete(change.index, change.deleteCount)
          if (change.insert) text.insert(change.index, change.insert)
        }
      }
      const same =
        entryLemmas(entry)?.size === 0 && applied.lemmas.length === 0
      if (!same) {
        const lemmas = new Y.Map<unknown>()
        for (const choice of applied.lemmas) {
          lemmas.set(`${choice.token}#${choice.occurrence}`, { key: choice.lemmaKey, extension: choice.extension })
        }
        entry.set('lemmas', lemmas)
      }
    }, LOCAL_ORIGIN),
  )
  return true
}

export interface PendingReview {
  entryId: string
  review: Review
}

type Listener = () => void

/** The review the dialog shows, per session. */
export class ReviewStore {
  private current: PendingReview | null = null
  private listeners = new Set<Listener>()
  getSnapshot = (): PendingReview | null => this.current
  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }
  set(value: PendingReview | null): void {
    this.current = value
    for (const listener of [...this.listeners]) listener()
  }
}

const stores = new WeakMap<AnalysisSession, ReviewStore>()
export function reviewStoreFor(session: AnalysisSession): ReviewStore {
  let store = stores.get(session)
  if (!store) stores.set(session, (store = new ReviewStore()))
  return store
}

export async function autoMacronize(services: EditorServices, entryId: string): Promise<void> {
  if (services.readOnly) {
    toast.info('You can’t edit this project.')
    return
  }
  const session = sessionFor(services.store)
  try {
    await session.start()
  } catch {
    toast.error('The Latin analyzer could not be loaded.')
    return
  }
  const client = session.api
  if (!client) return
  session.flush()
  let answer
  try {
    answer = await client.review(entryId)
  } catch {
    toast.error('The Latin analyzer failed. Try again.')
    return
  }
  const review = answer.value
  if (!review) {
    toast.info('Auto-macronize works on Latin entries.')
    return
  }
  if (review.direct) {
    if (review.direct.changed) writeReview(services.doc, services.undo, entryId, review.direct)
    else toast.info('Nothing to change.')
    return
  }
  reviewStoreFor(session).set({ entryId, review })
}

/** Apply the dialog's choices. Returns whether the dialog should close. */
export async function applyReviewChoices(
  services: EditorServices,
  pending: PendingReview,
  choices: (number | null)[],
): Promise<boolean> {
  const session = sessionFor(services.store)
  const client = session.api
  if (!client) return false
  session.flush()
  const answer = await client.applyReview(pending.entryId, pending.review.original, choices)
  if (!answer.value) {
    toast.error('The entry changed while you were reviewing it, so nothing was applied.')
    return true
  }
  if (!writeReview(services.doc, services.undo, pending.entryId, answer.value)) {
    toast.error('The entry no longer exists.')
  }
  return true
}
