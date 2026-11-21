// The project list: mine and shared, new project, import .loqr, and per
// project open / share / duplicate / delete or leave.
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useRef, useState } from 'react'
import { Link, useNavigate } from 'react-router'
import { api } from '../api/client'
import { queryKeys, useCurrentUser } from '../api/queries'
import type { ProjectSummary } from '../api/types'
import { ShareDialog } from '../sharing/ShareDialog'
import { Button } from '../ui/Button'
import { confirm, Dialog } from '../ui/Dialog'
import { FormError, TextField } from '../ui/Field'
import { More, Plus, Upload } from '../ui/icons'
import { MenuButton, type MenuItem } from '../ui/Menu'
import { EmptyState, PageLoading, RoleBadge, Time } from '../ui/misc'
import { errorMessage, toast } from '../ui/toasts'

export function projectTitle(name: string): string {
  return name.trim() || 'Untitled project'
}

export function ProjectsPage() {
  const user = useCurrentUser()
  const navigate = useNavigate()
  const client = useQueryClient()
  const projects = useQuery({ queryKey: queryKeys.projects, queryFn: api.projects.list })
  const [creating, setCreating] = useState(false)
  const [sharing, setSharing] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  const refresh = () => client.invalidateQueries({ queryKey: queryKeys.projects })
  const importProject = useMutation({
    mutationFn: (file: File) => api.projects.import(file),
    onSuccess: (project) => {
      void refresh()
      toast.success(`Imported “${projectTitle(project.name)}”.`)
      navigate(`/p/${project.id}`)
    },
    onError: (error) => toast.error(`Could not import the project: ${errorMessage(error)}`),
  })

  const duplicate = async (project: ProjectSummary) => {
    try {
      const copy = await api.projects.duplicate(project.id)
      void refresh()
      toast.success(`Duplicated as “${projectTitle(copy.name)}”.`)
    } catch (error) {
      toast.error(`Could not duplicate the project: ${errorMessage(error)}`)
    }
  }

  const remove = async (project: ProjectSummary) => {
    const owner = project.role === 'owner'
    const ok = await confirm(
      owner
        ? {
            title: `Delete “${projectTitle(project.name)}”?`,
            message: 'The project is removed for every collaborator. This cannot be undone from the project list.',
            confirmLabel: 'Delete project',
            danger: true,
          }
        : {
            title: `Leave “${projectTitle(project.name)}”?`,
            message: 'You’ll lose access until someone shares it with you again.',
            confirmLabel: 'Leave project',
            danger: true,
          },
    )
    if (!ok) return
    try {
      if (owner) await api.projects.remove(project.id)
      else await api.projects.removeMember(project.id, user.id)
      void refresh()
      toast.success(owner ? 'Project deleted.' : 'You left the project.')
    } catch (error) {
      toast.error(errorMessage(error))
    }
  }

  const list = projects.data ?? []
  const mine = list.filter((project) => project.role === 'owner')
  const shared = list.filter((project) => project.role !== 'owner')
  const sortByEdit = (a: ProjectSummary, b: ProjectSummary) => b.updatedAt.localeCompare(a.updatedAt)

  const menuFor = (project: ProjectSummary): MenuItem[] => [
    { label: 'Open', onSelect: () => navigate(`/p/${project.id}`) },
    { label: project.role === 'owner' ? 'Share…' : 'Collaborators…', onSelect: () => setSharing(project.id) },
    { label: 'Duplicate', onSelect: () => void duplicate(project), disabled: project.role === 'viewer', title: project.role === 'viewer' ? 'Viewers can’t duplicate' : undefined },
    { kind: 'separator' },
    project.role === 'owner'
      ? { label: 'Delete…', danger: true, onSelect: () => void remove(project) }
      : { label: 'Leave…', danger: true, onSelect: () => void remove(project) },
  ]

  const table = (items: ProjectSummary[], showOwner: boolean) => (
    <ul className="project-list">
      {[...items].sort(sortByEdit).map((project) => (
        <li key={project.id} className="project-row">
          <Link to={`/p/${project.id}`} className="project-row__main">
            <span className="project-row__name">{projectTitle(project.name)}</span>
            <span className="project-row__meta">
              {showOwner && <span>{project.owner.displayName}</span>}
              <span>
                {project.memberCount} {project.memberCount === 1 ? 'member' : 'members'}
              </span>
              <span>
                Edited <Time iso={project.updatedAt} />
              </span>
            </span>
          </Link>
          <RoleBadge role={project.role} />
          <MenuButton label={`Actions for ${projectTitle(project.name)}`} items={() => menuFor(project)}>
            <More />
          </MenuButton>
        </li>
      ))}
    </ul>
  )

  return (
    <div className="projects">
      <div className="page-header">
        <h1>Projects</h1>
        <div className="page-header__actions">
          <input
            ref={fileRef}
            type="file"
            accept=".loqr"
            hidden
            onChange={(event) => {
              const file = event.target.files?.[0]
              event.target.value = ''
              if (file) importProject.mutate(file)
            }}
          />
          <Button icon={<Upload />} busy={importProject.isPending} onClick={() => fileRef.current?.click()}>
            Import .loqr
          </Button>
          <Button variant="primary" icon={<Plus />} onClick={() => setCreating(true)}>
            New project
          </Button>
        </div>
      </div>

      {projects.isPending ? (
        <PageLoading />
      ) : projects.error ? (
        <FormError error={`Could not load your projects: ${errorMessage(projects.error)}`} />
      ) : list.length === 0 ? (
        <EmptyState
          title="No projects yet"
          action={
            <Button variant="primary" icon={<Plus />} onClick={() => setCreating(true)}>
              New project
            </Button>
          }
        >
          Create a project to start writing a lesson script, or import one from the desktop app.
        </EmptyState>
      ) : (
        <>
          <section aria-labelledby="mine-heading" className="projects__section">
            <h2 id="mine-heading">My projects</h2>
            {mine.length ? table(mine, false) : <p className="muted">You don’t own any projects yet.</p>}
          </section>
          <section aria-labelledby="shared-heading" className="projects__section">
            <h2 id="shared-heading">Shared with me</h2>
            {shared.length ? table(shared, true) : <p className="muted">Projects others share with you appear here.</p>}
          </section>
        </>
      )}

      <NewProjectDialog open={creating} onClose={() => setCreating(false)} />
      {sharing && <ShareDialog projectId={sharing} open onClose={() => setSharing(null)} />}
    </div>
  )
}

export function NewProjectDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate()
  const client = useQueryClient()
  const [name, setName] = useState('')
  const [l1Name, setL1Name] = useState('English')
  const create = useMutation({
    mutationFn: () => api.projects.create({ name: name.trim(), l1Name: l1Name.trim() || 'English' }),
    onSuccess: (project) => {
      void client.invalidateQueries({ queryKey: queryKeys.projects })
      onClose()
      setName('')
      setL1Name('English')
      // The editor opens the project details with the name focused.
      navigate(`/p/${project.id}?details=1`)
    },
  })
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="New project"
      size="sm"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" type="submit" form="new-project-form" busy={create.isPending}>
            Create project
          </Button>
        </>
      }
    >
      <form
        id="new-project-form"
        className="form"
        onSubmit={(event) => {
          event.preventDefault()
          create.mutate()
        }}
      >
        <TextField label="Project name" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Latin for beginners" />
        <TextField
          label="Learners’ language"
          value={l1Name}
          onChange={(e) => setL1Name(e.target.value)}
          hint="The language teachers use besides Latin (L1)."
          required
        />
        <FormError error={create.error ? errorMessage(create.error) : null} />
      </form>
    </Dialog>
  )
}
