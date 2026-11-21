// The analysis worker: loads loquaris-core (wasm) and answers requests in the
// order they arrive. Results go back as JSON strings, parsed by the client.
import init, { Analyzer } from '../wasm/pkg/loquaris_core_wasm.js'
import type { Request, RequestMessage, ResponseMessage } from './protocol.ts'

interface WorkerScope {
  onmessage: ((event: MessageEvent<RequestMessage>) => void) | null
  postMessage(message: ResponseMessage): void
}

const scope = self as unknown as WorkerScope
const post = (message: ResponseMessage) => scope.postMessage(message)

const ready: Promise<Analyzer | null> = init({
  module_or_path: new URL('../wasm/pkg/loquaris_core_wasm_bg.wasm', import.meta.url),
})
  .then(() => {
    post({ type: 'ready' })
    return new Analyzer()
  })
  .catch((error: unknown) => {
    post({ type: 'failed', error: String(error) })
    return null
  })

/** The result as JSON, or undefined for requests without one. */
function handle(analyzer: Analyzer, request: Request): string | undefined {
  switch (request.type) {
    case 'setProject':
      analyzer.setProject(JSON.stringify(request.project))
      return undefined
    case 'setStructure':
      analyzer.setStructure(JSON.stringify(request.lessons))
      return undefined
    case 'setEntry':
      analyzer.setEntry(JSON.stringify(request.entry))
      return undefined
    case 'removeEntry':
      analyzer.removeEntry(request.id)
      return undefined
    case 'setL1Name':
      analyzer.setL1Name(request.l1Name)
      return undefined
    case 'setGaps':
      analyzer.setGaps(JSON.stringify(request.gaps))
      return undefined
    case 'search':
      return analyzer.search(request.query, JSON.stringify(request.options))
    case 'firstUse':
      return analyzer.firstUse()
    case 'spacedRepetition':
      return analyzer.spacedRepetition()
    case 'newInLesson':
      return analyzer.newInLesson(request.lessonId)
    case 'review':
      return analyzer.review(request.entryId)
    case 'applyReview':
      return analyzer.applyReview(
        request.entryId,
        request.original,
        JSON.stringify(request.choices),
      )
    case 'macronize':
      return JSON.stringify(analyzer.macronize(request.text))
  }
}

scope.onmessage = (event) => {
  const { id, revision, request } = event.data
  // Handlers queue on the same promise, so requests keep their order.
  void ready.then((analyzer) => {
    if (!analyzer) {
      post({ type: 'error', id, revision, error: 'Analysis failed to load.', fatal: true })
      return
    }
    try {
      post({ type: 'result', id, revision, json: handle(analyzer, request) })
    } catch (error) {
      // A trap (a Rust panic) leaves the module unusable; bad input does not.
      const fatal = error instanceof WebAssembly.RuntimeError
      post({ type: 'error', id, revision, error: String(error), fatal })
    }
  })
}
