// The editor's top bar: back to projects, project name, connection status,
// collaborators, sharing and the project menu.
import { useSyncExternalStore, type CSSProperties } from 'react'
import { Link, useNavigate } from 'react-router'
import { STATUS_LABEL, useConnectionState } from '../collab/connection'
import { usePeers, type Peer } from '../collab/presence'
import { UserMenu } from '../pages/AppLayout'
import { Button } from '../ui/Button'
import { ArrowLeft, More, Share } from '../ui/icons'
import { MenuButton, type MenuItem } from '../ui/Menu'
import { Avatar, Badge } from '../ui/misc'
import { useEditor } from './context'
import { editorExtensions } from './extensions'

export function EditorHeader() {
  const services = useEditor()
  const { store, connection, presence, readOnly, commands, openProjectDetails, openShare, project } = services
  const navigate = useNavigate()
  const name = useSyncExternalStore(store.subscribeMeta, () => store.getMeta().name)
  const { status } = useConnectionState(connection)
  const peers = usePeers(presence)
  // One avatar per person, even with several tabs open.
  const people = new Map<string, Peer>()
  for (const peer of peers) if (!people.has(peer.user.id)) people.set(peer.user.id, peer)

  const menu = (): MenuItem[] => [
    { label: 'Project details…', onSelect: () => openProjectDetails() },
    { label: project.role === 'owner' ? 'Share…' : 'Collaborators…', onSelect: openShare },
    ...editorExtensions.projectMenuItems.flatMap((contribute) => contribute(services)),
    { kind: 'separator' },
    { label: 'Keyboard shortcuts…', onSelect: () => navigate('/settings#keys-heading') },
    { label: 'All projects', onSelect: () => navigate('/') },
  ]

  return (
    <header className="editor-header">
      <Link to="/" className="icon-btn icon-btn--ghost" aria-label="All projects" title="All projects">
        <ArrowLeft />
      </Link>
      <button type="button" className="editor-header__title" onClick={() => openProjectDetails()} title="Project details">
        {name.trim() || <span className="muted">Untitled project</span>}
      </button>
      {readOnly && <Badge>View only</Badge>}
      <span className={`status status--${status}`} role="status" aria-live="polite">
        <span className="status__dot" aria-hidden />
        {STATUS_LABEL[status]}
      </span>
      <div className="editor-header__end">
        <div className="presence" aria-label={people.size ? `Also here: ${Array.from(people.values(), (p) => p.user.name).join(', ')}` : undefined}>
          {Array.from(people.values()).map((peer) => (
            <button
              key={peer.user.id}
              type="button"
              className="presence__person"
              style={{ '--avatar-color': peer.user.color } as CSSProperties}
              title={peer.entry ? `${peer.user.name} — click to go to their entry` : peer.user.name}
              onClick={() => peer.entry && commands.reveal(peer.entry)}
            >
              <Avatar name={peer.user.name} userId={peer.user.id} color={peer.user.color} size="sm" />
              {peer.recording && <span className="presence__recording" aria-label="recording" />}
            </button>
          ))}
        </div>
        <Button icon={<Share />} onClick={openShare}>
          Share
        </Button>
        <MenuButton label="Project menu" items={menu}>
          <More />
        </MenuButton>
        <UserMenu />
      </div>
    </header>
  )
}
