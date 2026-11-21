import { useSyncExternalStore } from 'react'
import type { CardDecorationProps } from '../../editor/extensions'
import { sessionFor } from '../session'

const NONE: string[] = []

/** Lemmas first used in this entry (FEATURES §4): yellow, Latin serif. */
export function FirstUseLabels({ services, entry }: CardDecorationProps) {
  const session = sessionFor(services.store)
  const uses = useSyncExternalStore(session.firstUses.subscribe, session.firstUses.getSnapshot)
  const words = uses?.[entry.id] ?? NONE
  if (words.length === 0) return null
  return (
    <span className="first-use" title="Words first used here">
      {words.join(' · ')}
    </span>
  )
}
