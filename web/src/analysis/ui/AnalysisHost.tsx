import { useEffect } from 'react'
import type { EditorServices } from '../../editor/context'
import { sessionFor } from '../session'

/** Starts the worker once the editor has rendered and stops it on unmount. */
export function AnalysisHost({ services }: { services: EditorServices }) {
  useEffect(() => {
    const session = sessionFor(services.store)
    const timer = setTimeout(() => void session.start(), 0)
    return () => {
      clearTimeout(timer)
      session.dispose()
    }
  }, [services.store])
  return null
}
