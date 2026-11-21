// The collaboration connection: a y-websocket provider for one project plus a
// small status store for the header ("Saved", "Saving…", "Connecting…",
// "Offline — changes kept until reconnect").
import { useSyncExternalStore } from 'react'
import { WebsocketProvider } from 'y-websocket'
import * as Y from 'yjs'
import { urls } from '../api/client'

export type ConnectionStatus =
  | 'connecting' // never synced yet
  | 'saving' // connected and synced, local changes not yet sent
  | 'saved'
  | 'offline' // was connected; edits are kept and sent on reconnect
  | 'closed' // the server refused us for good (access revoked, project gone)

export interface ConnectionState {
  status: ConnectionStatus
  /** True once the first sync with the server finished. */
  synced: boolean
  /** Why the server closed the connection for good, when `status` is "closed". */
  reason: string | null
  /** The server's close code when `status` is "closed" (4401 session ended,
   * 4403 removed from project, 4404 project deleted, 4405 viewer edits). */
  code: number | null
}

/** Close codes after which reconnecting is pointless (crates/server). */
const TERMINAL_CLOSE: Record<number, string> = {
  4400: 'The server rejected a malformed message.',
  4401: 'Your session ended. Sign in again to keep editing.',
  4403: 'You were removed from this project.',
  4404: 'This project was deleted.',
  4405: 'Viewers can’t edit this project.',
}
/** The server changed our role (viewer ↔ editor): refetch the project, the
 * provider reconnects by itself. */
export const ROLE_CHANGED_CLOSE = 4001

export class Connection {
  readonly doc: Y.Doc
  readonly provider: WebsocketProvider
  private state: ConnectionState = { status: 'connecting', synced: false, reason: null, code: null }
  /** Called when the server reports a role change (close code 4001). */
  onRoleChanged: (() => void) | null = null
  private listeners = new Set<() => void>()
  private everSynced = false
  private dirty = false
  private flushTimer: ReturnType<typeof setInterval> | null = null
  private readonly disposers: (() => void)[] = []

  constructor(projectId: string, doc: Y.Doc, options: { WebSocketPolyfill?: typeof WebSocket } = {}) {
    this.doc = doc
    this.provider = new WebsocketProvider(urls.syncServer(), `${projectId}/sync`, doc, {
      // Tabs of one browser still sync through the server, so each tab's
      // status reflects what the server has.
      disableBc: true,
      maxBackoffTime: 5000,
      ...(options.WebSocketPolyfill ? { WebSocketPolyfill: options.WebSocketPolyfill } : {}),
    })
    const onStatus = () => this.recompute()
    const onSync = (synced: boolean) => {
      if (synced) this.everSynced = true
      this.recompute()
    }
    const onClosed = (event: { code: number; reason: string }) => {
      this.set({ status: 'closed', synced: this.everSynced, reason: TERMINAL_CLOSE[event.code] ?? (event.reason || null), code: event.code })
    }
    const onConnectionClose = (event: CloseEvent | null) => {
      if (!event) return
      if (event.code === ROLE_CHANGED_CLOSE) this.onRoleChanged?.()
      else if (TERMINAL_CLOSE[event.code]) {
        this.provider.disconnect()
        onClosed(event)
      }
    }
    const onUpdate = (_update: Uint8Array, origin: unknown) => {
      if (origin === this.provider) return
      this.dirty = true
      this.recompute()
      this.watchBuffer()
    }
    this.provider.on('status', onStatus)
    this.provider.on('sync', onSync)
    this.provider.on('closed', onClosed)
    this.provider.on('connection-close', onConnectionClose)
    doc.on('update', onUpdate)
    this.disposers.push(
      () => this.provider.off('status', onStatus),
      () => this.provider.off('sync', onSync),
      () => this.provider.off('closed', onClosed),
      () => this.provider.off('connection-close', onConnectionClose),
      () => doc.off('update', onUpdate),
    )
  }

  destroy(): void {
    for (const dispose of this.disposers) dispose()
    if (this.flushTimer) clearInterval(this.flushTimer)
    this.provider.destroy()
  }

  /** Poll the socket's send buffer until local changes have left. */
  private watchBuffer(): void {
    if (this.flushTimer) return
    this.flushTimer = setInterval(() => this.recompute(), 250)
  }

  private recompute(): void {
    if (this.state.status === 'closed') return
    const { provider } = this
    const connected = provider.wsconnected
    const synced = connected && provider.synced
    if (synced && this.dirty && (provider.ws?.bufferedAmount ?? 0) === 0) this.dirty = false
    if (!this.dirty && this.flushTimer) {
      clearInterval(this.flushTimer)
      this.flushTimer = null
    }
    let status: ConnectionStatus
    if (!connected) status = this.everSynced ? 'offline' : 'connecting'
    else if (!synced) status = this.everSynced ? 'saving' : 'connecting'
    else status = this.dirty ? 'saving' : 'saved'
    this.set({ status, synced: this.everSynced, reason: null, code: null })
  }

  private set(state: ConnectionState): void {
    if (state.status === this.state.status && state.synced === this.state.synced && state.reason === this.state.reason && state.code === this.state.code) return
    this.state = state
    for (const listener of this.listeners) listener()
  }

  getState = (): ConnectionState => this.state

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }
}

export function useConnectionState(connection: Connection): ConnectionState {
  return useSyncExternalStore(connection.subscribe, connection.getState)
}

export const STATUS_LABEL: Record<ConnectionStatus, string> = {
  connecting: 'Connecting…',
  saving: 'Saving…',
  saved: 'Saved',
  offline: 'Offline — changes kept until reconnect',
  closed: 'Disconnected',
}
