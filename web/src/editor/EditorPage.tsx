// /p/:projectId — the collaborative script editor.
import { useQuery } from '@tanstack/react-query'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Link, useParams, useSearchParams } from 'react-router'
import { api, ApiError } from '../api/client'
import { queryKeys, useCurrentUser, useSetMe } from '../api/queries'
import { useConnectionState } from '../collab/connection'
import { resolveKeymap } from '../keybindings/keybindings'
import { ShareDialog } from '../sharing/ShareDialog'
import { PageLoading } from '../ui/misc'
import { errorMessage, toast } from '../ui/toasts'
import { EditorContext, useEditor, type EditorServices } from './context'
import { EditorHeader } from './EditorHeader'
import { editorExtensions } from './extensions'
import './plugins'
import { ProjectDetailsDialog } from './ProjectDetailsDialog'
import { Script } from './Script'
import { useProjectSession } from './session'
import { useEditorKeys } from './useEditorKeys'

export function EditorPage() {
  const { projectId = '' } = useParams()
  const user = useCurrentUser()
  const project = useQuery({ queryKey: queryKeys.project(projectId), queryFn: () => api.projects.get(projectId), retry: false })
  const readOnly = project.data?.role === 'viewer'
  const servicesRef = useRef<EditorServices | null>(null)
  const session = useProjectSession(projectId, project.data ? user : null, readOnly, () => servicesRef.current)
  const [details, setDetails] = useState<{ open: boolean; focusName: boolean }>({ open: false, focusName: false })
  const [sharing, setSharing] = useState(false)
  const [params, setParams] = useSearchParams()

  // A new project opens its details with the name focused (FEATURES §1).
  useEffect(() => {
    if (params.get('details') && session) {
      setDetails({ open: true, focusName: true })
      setParams({}, { replace: true })
    }
  }, [params, session, setParams])

  const services = useMemo<EditorServices | null>(() => {
    if (!session || !project.data) return null
    return {
      ...session,
      projectId,
      project: project.data,
      user,
      readOnly,
      keymap: resolveKeymap(user.settings),
      openProjectDetails: (focus) => setDetails({ open: true, focusName: focus === 'name' }),
      openShare: () => setSharing(true),
    }
  }, [session, project.data, projectId, user, readOnly])
  useEffect(() => {
    servicesRef.current = services
  }, [services])
  useEditorKeys(services)

  // Role changed on the server (close 4001): refetch to update read-only.
  const refetchProject = project.refetch
  useEffect(() => {
    if (!session) return
    session.connection.onRoleChanged = () => void refetchProject()
    return () => {
      session.connection.onRoleChanged = null
    }
  }, [session, refetchProject])

  if (project.error) {
    const missing = project.error instanceof ApiError && (project.error.status === 404 || project.error.status === 403)
    return (
      <main className="auth">
        <div className="auth__panel">
          <h1 className="auth__title">{missing ? 'Project not found' : 'Could not open the project'}</h1>
          <p>{missing ? 'It may have been deleted, or you no longer have access.' : errorMessage(project.error)}</p>
          <p>
            <Link to="/">Go to your projects</Link>
          </p>
        </div>
      </main>
    )
  }
  if (!services) return <PageLoading label="Opening project…" />

  return (
    <EditorContext.Provider value={services}>
      <div className="editor">
        <EditorHeader />
        <ConnectionNotices />
        <div className={['editor__main', editorExtensions.sidePanels.length > 0 && 'editor__main--with-panel'].filter(Boolean).join(' ')}>
          <main className="editor__script" id="main" aria-label="Script">
            <SyncGate>
              <Script />
            </SyncGate>
          </main>
          {editorExtensions.sidePanels.length > 0 && (
            <aside className="side-panel" aria-label="Side panel">
              {editorExtensions.sidePanels.map((panel) => (
                <div key={panel.id} className="side-panel__section">
                  {panel.render(services)}
                </div>
              ))}
            </aside>
          )}
        </div>
        <ProjectDetailsDialog open={details.open} focusName={details.focusName} onClose={() => setDetails({ open: false, focusName: false })} />
        {sharing && <ShareDialog projectId={projectId} open onClose={() => setSharing(false)} />}
        {editorExtensions.editorMounts.map((Mount, i) => (
          <Mount key={i} services={services} />
        ))}
      </div>
    </EditorContext.Provider>
  )
}

/** Shows a loading state until the first sync, so an empty replica is never
 * mistaken for an empty project. */
function SyncGate({ children }: { children: ReactNode }) {
  const { connection } = useEditor()
  const { synced, status } = useConnectionState(connection)
  if (!synced) {
    return status === 'closed' ? null : <PageLoading label="Connecting to the project…" />
  }
  return <>{children}</>
}

function ConnectionNotices() {
  const { connection } = useEditor()
  const { status, reason, code } = useConnectionState(connection)
  const shown = useRef(false)
  const setMe = useSetMe()
  useEffect(() => {
    // Session ended: forget the user; RequireAuth sends them to sign in.
    if (status === 'closed' && code === 4401) setMe(null)
  }, [status, code, setMe])
  useEffect(() => {
    if (status === 'closed' && !shown.current) {
      shown.current = true
      toast.error(reason ? reason : 'The server closed the connection. You may no longer have access to this project.')
    }
  }, [status, reason])
  if (status !== 'closed') return null
  return (
    <div className="banner banner--error" role="alert">
      Disconnected from the project. Changes you make now won’t be saved. <button type="button" className="link-button" onClick={() => location.reload()}>Reload</button>
    </div>
  )
}
