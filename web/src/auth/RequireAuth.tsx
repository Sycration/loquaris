import { Navigate, Outlet, useLocation } from 'react-router'
import { useMe } from '../api/queries'
import { PageLoading } from '../ui/misc'
import { errorMessage } from '../ui/toasts'

/** Renders child routes only when signed in; otherwise redirects to
 * /login?next=<this path>. */
export function RequireAuth() {
  const location = useLocation()
  const { data: user, isPending, error, refetch } = useMe()
  if (isPending) return <PageLoading />
  if (error) {
    return (
      <div className="page-error" role="alert">
        <p>Could not reach the server: {errorMessage(error)}</p>
        <button type="button" className="btn btn--secondary" onClick={() => void refetch()}>
          Try again
        </button>
      </div>
    )
  }
  if (!user) {
    const next = location.pathname + location.search
    return <Navigate to={`/login${next === '/' ? '' : `?next=${encodeURIComponent(next)}`}`} replace />
  }
  return <Outlet />
}

/** A safe in-app return path from ?next=. */
export function safeNext(next: string | null): string {
  return next && next.startsWith('/') && !next.startsWith('//') ? next : '/'
}
