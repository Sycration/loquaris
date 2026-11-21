// Chrome for the project list, settings and admin pages.
import { useQueryClient } from '@tanstack/react-query'
import { useRef, useState } from 'react'
import { Link, NavLink, Outlet, useNavigate } from 'react-router'
import { api } from '../api/client'
import { queryKeys, useCurrentUser } from '../api/queries'
import { Menu } from '../ui/Menu'
import { Avatar } from '../ui/misc'
import { errorMessage, toast } from '../ui/toasts'

export function useSignOut() {
  const client = useQueryClient()
  const navigate = useNavigate()
  return async () => {
    try {
      await api.auth.logout()
    } catch (error) {
      toast.error(`Could not sign out: ${errorMessage(error)}`)
      return
    }
    client.clear()
    client.setQueryData(queryKeys.me, null)
    navigate('/login', { replace: true })
  }
}

export function UserMenu() {
  const user = useCurrentUser()
  const navigate = useNavigate()
  const signOut = useSignOut()
  const buttonRef = useRef<HTMLButtonElement>(null)
  const [position, setPosition] = useState<{ x: number; y: number } | null>(null)
  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        className="user-button"
        aria-haspopup="menu"
        aria-expanded={position !== null}
        aria-label={`Account: ${user.displayName}`}
        onClick={() => {
          const rect = buttonRef.current!.getBoundingClientRect()
          setPosition(position ? null : { x: rect.right - 200, y: rect.bottom + 6 })
        }}
      >
        <Avatar name={user.displayName} userId={user.id} size="sm" />
      </button>
      {position && (
        <Menu
          label="Account"
          position={position}
          returnFocus={buttonRef}
          onClose={() => setPosition(null)}
          items={[
            { label: 'Projects', onSelect: () => navigate('/') },
            { label: 'Settings', onSelect: () => navigate('/settings') },
            ...(user.isAdmin ? [{ label: 'Administration', onSelect: () => navigate('/admin') }] : []),
            { kind: 'separator' as const },
            { label: 'Sign out', onSelect: () => void signOut() },
          ]}
        />
      )}
    </>
  )
}

export function AppLayout() {
  const user = useCurrentUser()
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <header className="topbar">
        <Link to="/" className="topbar__brand">
          <span className="brand-mark" aria-hidden>
            L
          </span>
          Loquaris
        </Link>
        <nav className="topbar__nav" aria-label="Main">
          <NavLink to="/" end>
            Projects
          </NavLink>
          <NavLink to="/settings">Settings</NavLink>
          {user.isAdmin && <NavLink to="/admin">Admin</NavLink>}
        </nav>
        <div className="topbar__end">
          <UserMenu />
        </div>
      </header>
      <main id="main" className="page">
        <Outlet />
      </main>
    </div>
  )
}

export function NotFoundPage() {
  return (
    <main className="auth">
      <div className="auth__panel">
        <h1 className="auth__title">Page not found</h1>
        <p>There’s nothing at this address.</p>
        <p>
          <Link to="/">Go to your projects</Link>
        </p>
      </div>
    </main>
  )
}
