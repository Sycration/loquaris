// Administration: accounts, admin and disabled flags, set-password links.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { api } from '../api/client'
import { queryKeys, useConfig, useCurrentUser } from '../api/queries'
import type { AdminUser, ResetLinkResponse } from '../api/types'
import { Button } from '../ui/Button'
import { Dialog } from '../ui/Dialog'
import { Checkbox, FormError, TextField } from '../ui/Field'
import { Badge, CopyField, formatDate, PageLoading, Time } from '../ui/misc'
import { errorMessage, toast } from '../ui/toasts'

export function AdminPage() {
  const me = useCurrentUser()
  const client = useQueryClient()
  const { data: config } = useConfig()
  const users = useQuery({ queryKey: queryKeys.adminUsers, queryFn: api.admin.users, enabled: me.isAdmin })
  const [link, setLink] = useState<{ title: string; response: ResetLinkResponse } | null>(null)
  const [creating, setCreating] = useState(false)

  if (!me.isAdmin) {
    return (
      <div className="page-header">
        <h1>Administration</h1>
        <p className="muted">You need to be an administrator to see this page.</p>
      </div>
    )
  }

  const update = async (user: AdminUser, change: { isAdmin?: boolean; disabled?: boolean }) => {
    try {
      await api.admin.updateUser(user.id, change)
      void client.invalidateQueries({ queryKey: queryKeys.adminUsers })
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }
  const resetLink = async (user: AdminUser) => {
    try {
      const response = await api.admin.resetLink(user.id)
      setLink({ title: `Password link for ${user.displayName}`, response })
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  return (
    <div className="admin">
      <div className="page-header">
        <div>
          <h1>Administration</h1>
          {config && (
            <p className="muted">
              Registration is <strong>{config.registration === 'open' ? 'open to anyone' : config.registration === 'invite' ? 'by project invite only' : 'closed'}</strong>{' '}
              (set with <code>LOQ_REGISTRATION</code>). Server version {config.version}.
            </p>
          )}
        </div>
        <div className="page-header__actions">
          <Button variant="primary" onClick={() => setCreating(true)}>
            Create account
          </Button>
        </div>
      </div>
      {users.isPending ? (
        <PageLoading />
      ) : users.error ? (
        <FormError error={errorMessage(users.error)} />
      ) : (
        <div className="table-wrap">
          <table className="data-table">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Email</th>
                <th scope="col">Projects</th>
                <th scope="col">Created</th>
                <th scope="col">Admin</th>
                <th scope="col">Disabled</th>
                <th scope="col">
                  <span className="visually-hidden">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {users.data.map((user) => (
                <tr key={user.id} className={user.disabled ? 'is-disabled' : undefined}>
                  <td>
                    {user.displayName} {user.id === me.id && <Badge>you</Badge>}
                  </td>
                  <td>{user.email}</td>
                  <td>{user.projectCount}</td>
                  <td>
                    <Time iso={user.createdAt} />
                  </td>
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`${user.displayName} is an administrator`}
                      checked={user.isAdmin}
                      disabled={user.id === me.id}
                      onChange={(event) => void update(user, { isAdmin: event.target.checked })}
                    />
                  </td>
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`${user.displayName} is disabled`}
                      checked={user.disabled}
                      disabled={user.id === me.id}
                      onChange={(event) => void update(user, { disabled: event.target.checked })}
                    />
                  </td>
                  <td>
                    <Button size="sm" variant="ghost" onClick={() => void resetLink(user)}>
                      Reset link
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <CreateUserDialog
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(title, response) => {
          setCreating(false)
          setLink({ title, response })
          void client.invalidateQueries({ queryKey: queryKeys.adminUsers })
        }}
      />
      <Dialog
        open={link !== null}
        onClose={() => setLink(null)}
        title={link?.title ?? ''}
        description="Send this one-time link to the person. It lets them choose a password."
        footer={
          <Button variant="primary" onClick={() => setLink(null)}>
            Done
          </Button>
        }
      >
        {link && (
          <>
            <CopyField value={link.response.url} label="Set-password link" />
            <p className="muted small">Valid until {formatDate(link.response.expiresAt)}.</p>
          </>
        )}
      </Dialog>
    </div>
  )
}

function CreateUserDialog({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: (title: string, response: ResetLinkResponse) => void }) {
  const [email, setEmail] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [isAdmin, setIsAdmin] = useState(false)
  const create = useMutation({
    mutationFn: () => api.admin.createUser({ email: email.trim(), displayName: displayName.trim(), isAdmin }),
    onSuccess: (response) => {
      onCreated(`Account created for ${displayName.trim()}`, response)
      setEmail('')
      setDisplayName('')
      setIsAdmin(false)
    },
  })
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Create account"
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" form="create-user-form" busy={create.isPending}>
            Create
          </Button>
        </>
      }
    >
      <form
        id="create-user-form"
        className="form"
        onSubmit={(event) => {
          event.preventDefault()
          create.mutate()
        }}
      >
        <TextField label="Name" required value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
        <TextField label="Email" type="email" required value={email} onChange={(e) => setEmail(e.target.value)} />
        <Checkbox label="Administrator" checked={isAdmin} onChange={(e) => setIsAdmin(e.target.checked)} />
        <FormError error={create.error ? errorMessage(create.error) : null} />
      </form>
    </Dialog>
  )
}
