import { useEffect, useState } from 'react'
import type * as Y from 'yjs'

/** Whether undo / redo are available, kept current. */
export function useUndoState(undo: Y.UndoManager): { canUndo: boolean; canRedo: boolean } {
  const read = () => ({ canUndo: undo.canUndo(), canRedo: undo.canRedo() })
  const [state, setState] = useState(read)
  useEffect(() => {
    const update = () =>
      setState((previous) => {
        const next = read()
        return next.canUndo === previous.canUndo && next.canRedo === previous.canRedo ? previous : next
      })
    update()
    undo.on('stack-item-added', update)
    undo.on('stack-item-popped', update)
    undo.on('stack-cleared', update)
    return () => {
      undo.off('stack-item-added', update)
      undo.off('stack-item-popped', update)
      undo.off('stack-cleared', update)
    }
    // read closes over undo only.
  }, [undo]) // eslint-disable-line react-hooks/exhaustive-deps
  return state
}
