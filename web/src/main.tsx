import { QueryClientProvider } from '@tanstack/react-query'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createQueryClient } from './api/queries'
import App from './App'
import './styles/tokens.css'
import './styles/base.css'
import './styles/ui.css'
import './styles/pages.css'
import './styles/editor.css'

const queryClient = createQueryClient()

// A page loaded before a new version was deployed asks for code chunks that
// no longer exist; reload to get the current version. At most once every
// ten seconds, so a server that really lacks a file can't cause a loop.
window.addEventListener('vite:preloadError', (event) => {
  const key = 'loquaris:reloaded-for-update'
  try {
    const last = Number(sessionStorage.getItem(key) ?? 0)
    if (Date.now() - last < 10_000) return
    sessionStorage.setItem(key, String(Date.now()))
  } catch {
    return
  }
  event.preventDefault()
  location.reload()
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </StrictMode>,
)
