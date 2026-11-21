// Members and invite links of a project. Owners manage them; everyone else
// sees who has access and can leave.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { api } from '../api/client'
import { queryKeys, useCurrentUser } from '../api/queries'
import type { CreateInviteResponse, Invite, Member, Role } from '../api/types'
import { Button, IconButton } from '../ui/Button'
import { confirm, Dialog } from '../ui/Dialog'
import { FormError, SelectField, TextField } from '../ui/Field'
import { Trash } from '../ui/icons'
import { Avatar, CopyField, formatDate, PageLoading, RoleBadge, roleLabel } from '../ui/misc'
import { errorMessage, toast } from '../ui/toasts'

export function ShareDialog({ projectId, open, onClose }: { projectId: string; open: boolean; onClose: () => void }) {
  const me = useCurrentUser()
  const client = useQueryClient()
  const navigate = useNavigate()
  const project = useQuery({ queryKey: queryKeys.project(projectId), queryFn: () => api.projects.get(projectId), enabled: open, staleTime: 0 })
  const refresh = () => {
    void client.invalidateQueries({ queryKey: queryKeys.project(projectId) })
    void client.invalidateQueries({ queryKey: queryKeys.projects })
  }
  const isOwner = project.data?.role === 'owner'

  const [email, setEmail] = useState('')
  const [role, setRole] = useState<Exclude<Role, 'owner'>>('editor')
  const addMember = useMutation({
    mutationFn: () => api.projects.addMember(projectId, { email: email.trim(), role }),
    onSuccess: (member) => {
      setEmail('')
      refresh()
      toast.success(`${member.displayName} can now ${member.role === 'viewer' ? 'view' : 'edit'} the project.`)
    },
  })

  const changeRole = async (member: Member, next: Role) => {
    if (next === member.role) return
    if (next === 'owner') {
      const ok = await confirm({
        title: `Make ${member.displayName} the owner?`,
        message: 'You’ll become an editor and can no longer manage sharing, rename or delete the project.',
        confirmLabel: 'Transfer ownership',
        danger: true,
      })
      if (!ok) return
    }
    try {
      await api.projects.updateMember(projectId, member.userId, { role: next })
      refresh()
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  const removeMember = async (member: Member) => {
    const self = member.userId === me.id
    const ok = await confirm({
      title: self ? 'Leave this project?' : `Remove ${member.displayName}?`,
      message: self ? 'You’ll lose access until someone shares it with you again.' : 'They lose access to the project immediately.',
      confirmLabel: self ? 'Leave project' : 'Remove',
      danger: true,
    })
    if (!ok) return
    try {
      await api.projects.removeMember(projectId, member.userId)
      if (self) {
        void client.invalidateQueries({ queryKey: queryKeys.projects })
        onClose()
        navigate('/')
      } else refresh()
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title={isOwner ? 'Share project' : 'Collaborators'} size="lg">
      {project.isPending ? (
        <PageLoading />
      ) : project.error ? (
        <FormError error={errorMessage(project.error)} />
      ) : (
        <div className="share">
          {isOwner && (
            <form
              className="share__add"
              onSubmit={(event) => {
                event.preventDefault()
                addMember.mutate()
              }}
            >
              <TextField
                label="Add people by email"
                type="email"
                required
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                placeholder="name@example.org"
                hint="They need an account on this server. Otherwise, send an invite link."
                fieldClassName="share__email"
              />
              <SelectField label="Role" value={role} onChange={(e) => setRole(e.target.value as Exclude<Role, 'owner'>)} fieldClassName="share__role">
                <option value="editor">Editor</option>
                <option value="viewer">Viewer</option>
              </SelectField>
              <Button type="submit" variant="primary" busy={addMember.isPending} className="share__add-button">
                Add
              </Button>
              <FormError error={addMember.error ? errorMessage(addMember.error) : null} />
            </form>
          )}

          <section aria-labelledby="members-heading">
            <h3 id="members-heading" className="section-title">
              People with access
            </h3>
            <ul className="member-list">
              {project.data.members.map((member) => (
                <li key={member.userId} className="member">
                  <Avatar name={member.displayName} userId={member.userId} />
                  <div className="member__who">
                    <span className="member__name">
                      {member.displayName}
                      {member.userId === me.id && <span className="muted"> (you)</span>}
                    </span>
                    <span className="member__email">{member.email}</span>
                  </div>
                  {isOwner && member.role !== 'owner' ? (
                    <select
                      className="input select select--compact"
                      aria-label={`Role of ${member.displayName}`}
                      value={member.role}
                      onChange={(event) => void changeRole(member, event.target.value as Role)}
                    >
                      <option value="editor">Editor</option>
                      <option value="viewer">Viewer</option>
                      <option value="owner">Owner (transfer)</option>
                    </select>
                  ) : (
                    <RoleBadge role={member.role} />
                  )}
                  {member.role !== 'owner' && (isOwner || member.userId === me.id) ? (
                    <IconButton
                      size="sm"
                      label={member.userId === me.id ? 'Leave project' : `Remove ${member.displayName}`}
                      onClick={() => void removeMember(member)}
                    >
                      <Trash />
                    </IconButton>
                  ) : (
                    <span className="member__spacer" />
                  )}
                </li>
              ))}
            </ul>
          </section>

          {isOwner && <InviteLinks projectId={projectId} invites={project.data.invites ?? []} onChange={refresh} />}
        </div>
      )}
    </Dialog>
  )
}

const EXPIRY_OPTIONS = [
  { value: '1', label: '1 day' },
  { value: '7', label: '7 days' },
  { value: '30', label: '30 days' },
  { value: '', label: 'Never' },
]

function InviteLinks({ projectId, invites, onChange }: { projectId: string; invites: Invite[]; onChange: () => void }) {
  const [role, setRole] = useState<Exclude<Role, 'owner'>>('editor')
  const [expiry, setExpiry] = useState('7')
  const [maxUses, setMaxUses] = useState('')
  const [created, setCreated] = useState<CreateInviteResponse | null>(null)
  const create = useMutation({
    mutationFn: () =>
      api.projects.createInvite(projectId, {
        role,
        expiresInDays: expiry ? Number(expiry) : null,
        maxUses: maxUses ? Math.max(1, Math.floor(Number(maxUses))) : null,
      }),
    onSuccess: (response) => {
      setCreated(response)
      onChange()
    },
  })
  const revoke = async (invite: Invite) => {
    const ok = await confirm({ title: 'Revoke this invite link?', message: 'People who haven’t used it yet won’t be able to join with it.', confirmLabel: 'Revoke', danger: true })
    if (!ok) return
    try {
      await api.projects.revokeInvite(projectId, invite.id)
      if (created?.invite.id === invite.id) setCreated(null)
      onChange()
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }
  return (
    <section aria-labelledby="invites-heading" className="invites">
      <h3 id="invites-heading" className="section-title">
        Invite links
      </h3>
      <p className="muted small">Anyone with a link can join with its role, including people who don’t have an account yet.</p>
      <form
        className="invites__form"
        onSubmit={(event) => {
          event.preventDefault()
          create.mutate()
        }}
      >
        <SelectField label="Role" value={role} onChange={(e) => setRole(e.target.value as Exclude<Role, 'owner'>)}>
          <option value="editor">Editor</option>
          <option value="viewer">Viewer</option>
        </SelectField>
        <SelectField label="Expires after" value={expiry} onChange={(e) => setExpiry(e.target.value)}>
          {EXPIRY_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </SelectField>
        <TextField label="Max uses" type="number" min={1} step={1} placeholder="Unlimited" value={maxUses} onChange={(e) => setMaxUses(e.target.value)} />
        <Button type="submit" busy={create.isPending} className="invites__create">
          Create link
        </Button>
      </form>
      <FormError error={create.error ? errorMessage(create.error) : null} />
      {created && (
        <div className="invites__created">
          <p className="small">New {roleLabel(created.invite.role).toLowerCase()} link — copy it now, it won’t be shown again:</p>
          <CopyField value={created.url} label="Invite link" />
        </div>
      )}
      {invites.length > 0 && (
        <ul className="invite-list">
          {invites.map((invite) => (
            <li key={invite.id} className="invite">
              <RoleBadge role={invite.role} />
              <span className="invite__details">
                {invite.uses}
                {invite.maxUses !== null ? ` of ${invite.maxUses}` : ''} {invite.uses === 1 && invite.maxUses === null ? 'use' : 'uses'} ·{' '}
                {invite.expiresAt ? `expires ${formatDate(invite.expiresAt)}` : 'never expires'} · by {invite.createdBy.displayName}
              </span>
              <Button size="sm" variant="ghost" onClick={() => void revoke(invite)}>
                Revoke
              </Button>
            </li>
          ))}
        </ul>
      )}
    </section>
  )
}
