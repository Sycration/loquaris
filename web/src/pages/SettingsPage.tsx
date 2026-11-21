// Account settings: profile, password, sessions, keybindings.
// TODO(audio agent): an "Audio input" section (device chosen in the recorder,
// remembered per browser) can be added as another <section> here.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import { api } from '../api/client'
import { queryKeys, useCurrentUser, useSetMe } from '../api/queries'
import type { Session } from '../api/types'
import { AudioInputSetting } from '../audio/AudioInputSetting'
import { KeybindingEditor } from '../keybindings/KeybindingEditor'
import { resolveKeymap } from '../keybindings/keybindings'
import { Button } from '../ui/Button'
import { confirm } from '../ui/Dialog'
import { FormError, TextField } from '../ui/Field'
import { Badge, PageLoading, Time } from '../ui/misc'
import { errorMessage, toast } from '../ui/toasts'
import { MIN_PASSWORD_LENGTH } from './AuthPages'

export function SettingsPage() {
  return (
    <div className="settings">
      <div className="page-header">
        <h1>Settings</h1>
      </div>
      <ProfileSection />
      <PasswordSection />
      <SessionsSection />
      <AudioInputSection />
      <KeybindingsSection />
    </div>
  )
}

function Section({ title, description, children, id }: { title: string; description?: string; children: ReactNode; id: string }) {
  return (
    <section className="settings-section" aria-labelledby={id}>
      <div className="settings-section__intro">
        <h2 id={id}>{title}</h2>
        {description && <p className="muted">{description}</p>}
      </div>
      <div className="settings-section__body">{children}</div>
    </section>
  )
}

function ProfileSection() {
  const user = useCurrentUser()
  const setMe = useSetMe()
  const [name, setName] = useState(user.displayName)
  const save = useMutation({
    mutationFn: () => api.auth.updateMe({ displayName: name.trim() }),
    onSuccess: (updated) => {
      setMe(updated)
      toast.success('Profile saved.')
    },
  })
  return (
    <Section id="profile-heading" title="Profile" description="Your name is shown to collaborators.">
      <form
        className="form form--narrow"
        onSubmit={(event) => {
          event.preventDefault()
          save.mutate()
        }}
      >
        <TextField label="Email" value={user.email} readOnly disabled hint="Ask an administrator to change your email." />
        <TextField label="Name" required value={name} onChange={(e) => setName(e.target.value)} autoComplete="name" />
        <FormError error={save.error ? errorMessage(save.error) : null} />
        <div>
          <Button type="submit" variant="primary" busy={save.isPending} disabled={!name.trim() || name.trim() === user.displayName}>
            Save
          </Button>
        </div>
      </form>
    </Section>
  )
}

function PasswordSection() {
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirmNext, setConfirmNext] = useState('')
  const [formError, setFormError] = useState<string | null>(null)
  const change = useMutation({
    mutationFn: () => api.auth.changePassword({ current, new: next }),
    onSuccess: () => {
      setCurrent('')
      setNext('')
      setConfirmNext('')
      toast.success('Password changed.')
    },
  })
  return (
    <Section id="password-heading" title="Password">
      <form
        className="form form--narrow"
        onSubmit={(event) => {
          event.preventDefault()
          if (next.length < MIN_PASSWORD_LENGTH) return setFormError(`Use at least ${MIN_PASSWORD_LENGTH} characters for the password.`)
          if (next !== confirmNext) return setFormError('The new passwords don’t match.')
          setFormError(null)
          change.mutate()
        }}
      >
        <TextField label="Current password" type="password" autoComplete="current-password" required value={current} onChange={(e) => setCurrent(e.target.value)} />
        <TextField label="New password" type="password" autoComplete="new-password" required value={next} onChange={(e) => setNext(e.target.value)} />
        <TextField label="Confirm new password" type="password" autoComplete="new-password" required value={confirmNext} onChange={(e) => setConfirmNext(e.target.value)} />
        <FormError error={formError ?? (change.error ? errorMessage(change.error) : null)} />
        <div>
          <Button type="submit" variant="primary" busy={change.isPending}>
            Change password
          </Button>
        </div>
      </form>
    </Section>
  )
}

function describeAgent(agent: string | null): string {
  if (!agent) return 'Unknown device'
  const browser =
    /Firefox\/\d+/.exec(agent)?.[0] ??
    /Edg\/\d+/.exec(agent)?.[0] ??
    /Chrome\/\d+/.exec(agent)?.[0] ??
    (/Version\/\d+.*Safari/.test(agent) ? 'Safari' : null)
  const os = /Windows/.test(agent) ? 'Windows' : /Mac OS X/.test(agent) ? 'macOS' : /Android/.test(agent) ? 'Android' : /iPhone|iPad/.test(agent) ? 'iOS' : /Linux/.test(agent) ? 'Linux' : null
  return [browser?.replace('/', ' ').replace('Edg', 'Edge'), os].filter(Boolean).join(' on ') || agent
}

function SessionsSection() {
  const client = useQueryClient()
  const navigate = useNavigate()
  const sessions = useQuery({ queryKey: queryKeys.sessions, queryFn: api.auth.sessions })
  const refresh = () => client.invalidateQueries({ queryKey: queryKeys.sessions })
  const revoke = async (session: Session) => {
    try {
      await api.auth.revokeSession(session.id)
      if (session.current) {
        client.clear()
        client.setQueryData(queryKeys.me, null)
        navigate('/login', { replace: true })
      } else void refresh()
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }
  const revokeOthers = async () => {
    const ok = await confirm({ title: 'Sign out everywhere else?', message: 'Every other browser signed in to your account is signed out.', confirmLabel: 'Sign out others' })
    if (!ok) return
    try {
      await api.auth.revokeOtherSessions()
      void refresh()
      toast.success('Signed out of all other sessions.')
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }
  const list = sessions.data ?? []
  return (
    <Section id="sessions-heading" title="Sessions" description="Browsers where you’re signed in.">
      {sessions.isPending ? (
        <PageLoading />
      ) : sessions.error ? (
        <FormError error={errorMessage(sessions.error)} />
      ) : (
        <>
          <ul className="session-list">
            {list.map((session) => (
              <li key={session.id} className="session">
                <div className="session__who">
                  <span>
                    {describeAgent(session.userAgent)} {session.current && <Badge tone="success">This browser</Badge>}
                  </span>
                  <span className="muted small">
                    Last active <Time iso={session.lastSeenAt} /> · signed in <Time iso={session.createdAt} />
                  </span>
                </div>
                <Button size="sm" variant="ghost" onClick={() => void revoke(session)}>
                  Sign out
                </Button>
              </li>
            ))}
          </ul>
          <Button onClick={() => void revokeOthers()} disabled={list.filter((s) => !s.current).length === 0}>
            Sign out other sessions
          </Button>
        </>
      )}
    </Section>
  )
}

function AudioInputSection() {
  return (
    <Section id="audio-heading" title="Audio input" description="The microphone the recorder uses. It is remembered in this browser only.">
      <AudioInputSetting />
    </Section>
  )
}

function KeybindingsSection() {
  const user = useCurrentUser()
  const setMe = useSetMe()
  const keymap = resolveKeymap(user.settings)
  return (
    <Section id="keys-heading" title="Keyboard shortcuts" description="Click a shortcut, then press the keys you want. Escape cancels.">
      <KeybindingEditor
        keymap={keymap}
        onSave={async (custom) => {
          try {
            const updated = await api.auth.updateMe({ settings: { ...user.settings, keybindings: custom } })
            setMe(updated)
          } catch (error) {
            toast.error(`Could not save shortcuts: ${errorMessage(error)}`)
          }
        }}
      />
    </Section>
  )
}
