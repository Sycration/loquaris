// Version history (DESIGN.md, Persistence): list, name the current state,
// restore. Restoring arrives as a document change over the sync socket.
import { useCallback, useEffect, useState } from 'react'
import { api } from '../api/client'
import type { Version } from '../api/types'
import type { EditorServices } from '../editor/context'
import { Button } from '../ui/Button'
import { Dialog, confirm } from '../ui/Dialog'
import { TextField } from '../ui/Field'
import { Spinner, formatDate } from '../ui/misc'
import { errorMessage, toast } from '../ui/toasts'
import { setHistoryOpen, useHistoryOpen } from './store'

export function HistoryMount({ services }: { services: EditorServices }) {
  const open = useHistoryOpen()
  return open ? <VersionHistoryDialog services={services} /> : null
}

function VersionHistoryDialog({ services }: { services: EditorServices }) {
  const { projectId, readOnly } = services
  const [versions, setVersions] = useState<Version[] | null>(null)
  const [label, setLabel] = useState('')
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    try {
      setVersions(await api.projects.versions(projectId))
    } catch (error) {
      toast.error(errorMessage(error))
      setVersions((current) => current ?? [])
    }
  }, [projectId])

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void load()
  }, [load])

  const name = async () => {
    const text = label.trim()
    if (!text) return
    setBusy(true)
    try {
      await api.projects.createVersion(projectId, { label: text })
      setLabel('')
      toast.success('Version named.')
      await load()
    } catch (error) {
      toast.error(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  const restore = async (version: Version) => {
    const title = version.label ?? 'this automatic version'
    const ok = await confirm({
      title: 'Restore this version?',
      message: `The project will be restored to “${title}” (${formatDate(version.createdAt)}). The current state is saved first as “Before restore”.`,
      confirmLabel: 'Restore',
      danger: true,
    })
    if (!ok) return
    setBusy(true)
    try {
      await api.projects.restoreVersion(projectId, version.id)
      toast.success('Version restored.')
      await load()
    } catch (error) {
      toast.error(errorMessage(error))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open onClose={() => setHistoryOpen(false)} title="Version history" size="lg" footer={<Button variant="ghost" onClick={() => setHistoryOpen(false)}>Close</Button>}>
      {!readOnly && (
        <form
          className="history-name"
          onSubmit={(event) => {
            event.preventDefault()
            void name()
          }}
        >
          <TextField label="Name current version" value={label} maxLength={200} onChange={(e) => setLabel(e.target.value)} fieldClassName="history-name__field" />
          <Button type="submit" variant="primary" disabled={busy || !label.trim()}>
            Save
          </Button>
        </form>
      )}
      {versions === null ? (
        <Spinner />
      ) : versions.length === 0 ? (
        <p className="export-note">No versions yet.</p>
      ) : (
        <ul className="history-list">
          {versions.map((version) => (
            <li key={version.id} className="history-item">
              <div className="history-item__main">
                <strong>{version.label ?? 'Automatic version'}</strong>
                <span className="history-item__meta">
                  {formatDate(version.createdAt)}
                  {version.createdBy ? ` · ${version.createdBy.displayName}` : ''}
                </span>
              </div>
              {!readOnly && (
                <Button size="sm" onClick={() => void restore(version)} disabled={busy}>
                  Restore
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
    </Dialog>
  )
}
