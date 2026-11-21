// Sign in, registration and password reset.
import { useMutation } from '@tanstack/react-query'
import { useState, type FormEvent, type ReactNode } from 'react'
import { Link, Navigate, useNavigate, useParams, useSearchParams } from 'react-router'
import { api } from '../api/client'
import { useConfig, useMe, useSetMe } from '../api/queries'
import { safeNext } from '../auth/RequireAuth'
import { Button } from '../ui/Button'
import { FormError, TextField } from '../ui/Field'
import { PageLoading } from '../ui/misc'
import { errorMessage } from '../ui/toasts'

export const MIN_PASSWORD_LENGTH = 8

export function AuthLayout({ title, subtitle, children, footer }: { title: string; subtitle?: ReactNode; children: ReactNode; footer?: ReactNode }) {
  return (
    <main className="auth">
      <div className="auth__panel">
        <div className="auth__brand" aria-hidden>
          <span className="brand-mark">L</span> Loquaris
        </div>
        <h1 className="auth__title">{title}</h1>
        {subtitle && <p className="auth__subtitle">{subtitle}</p>}
        {children}
        {footer && <div className="auth__footer">{footer}</div>}
      </div>
    </main>
  )
}

export function LoginPage() {
  const [params] = useSearchParams()
  const next = safeNext(params.get('next'))
  const navigate = useNavigate()
  const setMe = useSetMe()
  const { data: me } = useMe()
  const { data: config } = useConfig()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const login = useMutation({
    mutationFn: () => api.auth.login({ email: email.trim(), password }),
    onSuccess: (user) => {
      setMe(user)
      navigate(next, { replace: true })
    },
  })
  if (me) return <Navigate to={next} replace />
  const inviteToken = next.startsWith('/invite/') ? next.slice('/invite/'.length) : null
  const canRegister = config?.registration === 'open' || (config?.registration === 'invite' && inviteToken)
  return (
    <AuthLayout
      title="Sign in"
      subtitle="Write and record spoken Latin lessons together."
      footer={
        <>
          {canRegister && (
            <p>
              New here?{' '}
              <Link to={`/register?next=${encodeURIComponent(next)}${inviteToken ? `&invite=${encodeURIComponent(inviteToken)}` : ''}`}>
                Create an account
              </Link>
            </p>
          )}
          <p className="muted">Forgot your password? Ask an administrator for a reset link.</p>
        </>
      }
    >
      <form
        className="form"
        onSubmit={(event: FormEvent) => {
          event.preventDefault()
          login.mutate()
        }}
      >
        <TextField label="Email" type="email" autoComplete="email" required value={email} onChange={(e) => setEmail(e.target.value)} />
        <TextField
          label="Password"
          type="password"
          autoComplete="current-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <FormError error={login.error ? errorMessage(login.error) : null} />
        <Button type="submit" variant="primary" busy={login.isPending} className="btn--block">
          Sign in
        </Button>
      </form>
    </AuthLayout>
  )
}

export function RegisterPage() {
  const [params] = useSearchParams()
  const next = safeNext(params.get('next'))
  const invite = params.get('invite')
  const navigate = useNavigate()
  const setMe = useSetMe()
  const { data: me } = useMe()
  const { data: config, isPending } = useConfig()
  const [email, setEmail] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [password, setPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [formError, setFormError] = useState<string | null>(null)
  const register = useMutation({
    mutationFn: async () => {
      const user = await api.auth.register({
        email: email.trim(),
        displayName: displayName.trim(),
        password,
        ...(invite ? { invite } : {}),
      })
      let destination = next
      if (invite) {
        try {
          const { projectId } = await api.invites.accept(invite)
          destination = `/p/${projectId}`
        } catch {
          // Registration may already have used the invite; the invite page
          // explains anything left to do.
          destination = `/invite/${encodeURIComponent(invite)}`
        }
      }
      return { user, destination }
    },
    onSuccess: ({ user, destination }) => {
      setMe(user)
      navigate(destination, { replace: true })
    },
  })
  if (me) return <Navigate to={next} replace />
  if (isPending) return <PageLoading />
  const loginLink = (
    <p>
      Already have an account? <Link to={`/login?next=${encodeURIComponent(invite ? `/invite/${invite}` : next)}`}>Sign in</Link>
    </p>
  )
  if (config?.registration === 'closed') {
    return (
      <AuthLayout title="Registration is closed" footer={loginLink}>
        <p>Accounts on this server are created by administrators. Ask one to create an account for you.</p>
      </AuthLayout>
    )
  }
  if (config?.registration === 'invite' && !invite) {
    return (
      <AuthLayout title="An invite is needed" footer={loginLink}>
        <p>This server only accepts new accounts through a project invite link. Ask a project owner to send you one.</p>
      </AuthLayout>
    )
  }
  const submit = (event: FormEvent) => {
    event.preventDefault()
    if (password.length < MIN_PASSWORD_LENGTH) return setFormError(`Use at least ${MIN_PASSWORD_LENGTH} characters for the password.`)
    if (password !== confirmPassword) return setFormError('The passwords don’t match.')
    setFormError(null)
    register.mutate()
  }
  return (
    <AuthLayout title="Create an account" subtitle={invite ? 'You’ll join the project you were invited to.' : undefined} footer={loginLink}>
      <form className="form" onSubmit={submit}>
        <TextField label="Name" autoComplete="name" required value={displayName} onChange={(e) => setDisplayName(e.target.value)} hint="Shown to your collaborators." />
        <TextField label="Email" type="email" autoComplete="email" required value={email} onChange={(e) => setEmail(e.target.value)} />
        <TextField
          label="Password"
          type="password"
          autoComplete="new-password"
          required
          minLength={MIN_PASSWORD_LENGTH}
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          hint={`At least ${MIN_PASSWORD_LENGTH} characters.`}
        />
        <TextField label="Confirm password" type="password" autoComplete="new-password" required value={confirmPassword} onChange={(e) => setConfirmPassword(e.target.value)} />
        <FormError error={formError ?? (register.error ? errorMessage(register.error) : null)} />
        <Button type="submit" variant="primary" busy={register.isPending} className="btn--block">
          Create account
        </Button>
      </form>
    </AuthLayout>
  )
}

export function ResetPasswordPage() {
  const { token = '' } = useParams()
  const navigate = useNavigate()
  const setMe = useSetMe()
  const [password, setPassword] = useState('')
  const [confirmPassword, setConfirmPassword] = useState('')
  const [formError, setFormError] = useState<string | null>(null)
  const reset = useMutation({
    mutationFn: () => api.auth.resetPassword({ token, password }),
    onSuccess: (user) => {
      setMe(user)
      navigate('/', { replace: true })
    },
  })
  return (
    <AuthLayout title="Set a new password" subtitle="Choose the password you’ll sign in with.">
      <form
        className="form"
        onSubmit={(event) => {
          event.preventDefault()
          if (password.length < MIN_PASSWORD_LENGTH) return setFormError(`Use at least ${MIN_PASSWORD_LENGTH} characters for the password.`)
          if (password !== confirmPassword) return setFormError('The passwords don’t match.')
          setFormError(null)
          reset.mutate()
        }}
      >
        <TextField label="New password" type="password" autoComplete="new-password" required value={password} onChange={(e) => setPassword(e.target.value)} />
        <TextField label="Confirm password" type="password" autoComplete="new-password" required value={confirmPassword} onChange={(e) => setConfirmPassword(e.target.value)} />
        <FormError error={formError ?? (reset.error ? errorMessage(reset.error) : null)} />
        <Button type="submit" variant="primary" busy={reset.isPending} className="btn--block">
          Set password
        </Button>
      </form>
    </AuthLayout>
  )
}
