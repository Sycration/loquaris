import { useEffect, useState, useSyncExternalStore } from 'react'
import type { Answer } from '../client.ts'
import type { EditorServices } from '../../editor/context'
import { sessionFor, type AnalysisApi, type AnalysisSession } from '../session'

export function useSession(services: EditorServices): AnalysisSession {
  return sessionFor(services.store)
}

/** Re-renders when the session's status or content version changes. */
export function useSessionVersion(session: AnalysisSession): number {
  return useSyncExternalStore(session.subscribe, () => session.version)
}

export function useSearchState(session: AnalysisSession) {
  return useSyncExternalStore(session.search.subscribe, session.search.getState)
}

export function useSearchResult(session: AnalysisSession) {
  return useSyncExternalStore(session.search.subscribe, session.search.getResult)
}

/** A query re-run whenever the content changes; the last value is kept
 * while the next is computed, and stale answers are dropped. */
export function useAnalysisQuery<T>(
  session: AnalysisSession,
  key: string,
  run: (client: AnalysisApi) => Promise<Answer<T>>,
): T | null {
  const version = useSessionVersion(session)
  const [state, setState] = useState<{ key: string; value: T } | null>(null)
  useEffect(() => {
    const client = session.api
    if (!client || session.status !== 'ready') return
    let cancelled = false
    run(client).then(
      (answer) => {
        if (!cancelled && client.isCurrent(answer)) setState({ key, value: answer.value })
      },
      () => {},
    )
    return () => {
      cancelled = true
    }
    // `run` is derived from `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [session, version, key])
  return state && state.key === key ? state.value : null
}
