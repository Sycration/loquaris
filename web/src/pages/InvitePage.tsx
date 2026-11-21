// /invite/:token — shows what the invite is for and joins the project.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Link, useNavigate, useParams } from 'react-router'
import { api } from '../api/client'
import { queryKeys, useConfig, useMe } from '../api/queries'
import { Button } from '../ui/Button'
import { FormError } from '../ui/Field'
import { PageLoading, roleLabel } from '../ui/misc'
import { errorMessage } from '../ui/toasts'
import { AuthLayout } from './AuthPages'

export function InvitePage() {
  const { token = '' } = useParams()
  const navigate = useNavigate()
  const client = useQueryClient()
  const { data: me, isPending: mePending } = useMe()
  const { data: config } = useConfig()
  const preview = useQuery({ queryKey: queryKeys.invite(token), queryFn: () => api.invites.preview(token), retry: false })
  const accept = useMutation({
    mutationFn: () => api.invites.accept(token),
    onSuccess: ({ projectId }) => {
      void client.invalidateQueries({ queryKey: queryKeys.projects })
      navigate(`/p/${projectId}`, { replace: true })
    },
  })

  if (preview.isPending || mePending) return <PageLoading />
  if (preview.error || !preview.data) {
    return (
      <AuthLayout title="Invite not found" footer={<Link to="/">Go to your projects</Link>}>
        <p>This invite link doesn’t work. It may have been mistyped or revoked.</p>
      </AuthLayout>
    )
  }
  const invite = preview.data
  if (!invite.valid) {
    return (
      <AuthLayout title="This invite has expired" footer={<Link to="/">Go to your projects</Link>}>
        <p>
          The invite to <strong>{invite.projectName || 'Untitled project'}</strong> is no longer valid. Ask {invite.invitedBy.displayName} for a new
          link.
        </p>
      </AuthLayout>
    )
  }
  const next = encodeURIComponent(`/invite/${token}`)
  const canRegister = config?.registration !== 'closed'
  return (
    <AuthLayout
      title={`Join “${invite.projectName || 'Untitled project'}”`}
      subtitle={
        <>
          {invite.invitedBy.displayName} invited you as {invite.role === 'editor' ? 'an' : 'a'} <strong>{roleLabel(invite.role).toLowerCase()}</strong>
          {invite.role === 'viewer' ? ': you’ll be able to open, play and export the project.' : ': you’ll be able to edit the script and record.'}
        </>
      }
    >
      {me ? (
        <div className="form">
          <p className="muted">Signed in as {me.email}.</p>
          <FormError error={accept.error ? errorMessage(accept.error) : null} />
          <Button variant="primary" className="btn--block" busy={accept.isPending} onClick={() => accept.mutate()} data-autofocus>
            Join project
          </Button>
        </div>
      ) : (
        <div className="form">
          <Link className="btn btn--primary btn--block" to={`/login?next=${next}`}>
            Sign in to join
          </Link>
          {canRegister && (
            <Link className="btn btn--secondary btn--block" to={`/register?invite=${encodeURIComponent(token)}&next=${next}`}>
              Create an account
            </Link>
          )}
        </div>
      )}
    </AuthLayout>
  )
}
