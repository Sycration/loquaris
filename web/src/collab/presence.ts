// Presence over the Yjs awareness protocol (DESIGN.md, "Presence"): who is
// here, which entry or lesson name each person is in, their text selection
// as Y relative positions, and whether they are recording.
import { useCallback, useSyncExternalStore } from 'react'
import * as Y from 'yjs'
import type { Awareness } from 'y-protocols/awareness'

export interface PresenceUser {
  id: string
  name: string
  color: string
}

/** What a text selection is in. */
export type TextTarget = { kind: 'entry'; id: string } | { kind: 'lesson'; id: string }

export interface PresenceSelection {
  target: TextTarget
  /** Y.RelativePosition JSON (Y.relativePositionToJSON). */
  anchor: unknown
  head: unknown
}

export interface PresenceState {
  user: PresenceUser
  /** The entry this person has selected or is editing. */
  entry: string | null
  selection: PresenceSelection | null
  /** Entry ID being recorded, set by the recorder (audio agent). */
  recording: string | null
}

export interface Peer {
  clientId: number
  user: PresenceUser
  entry: string | null
  selection: PresenceSelection | null
  recording: string | null
}

// Distinct, legible on the dark theme, and not the card teal/red.
const PALETTE = ['#f5a524', '#a78bfa', '#34d399', '#f472b6', '#60a5fa', '#facc15', '#fb7185', '#2dd4bf', '#c084fc', '#a3e635', '#fdba74', '#93c5fd']

/** A stable colour per user, the same in every browser. */
export function colorForUser(userId: string): string {
  let hash = 2166136261
  for (let i = 0; i < userId.length; i++) {
    hash ^= userId.charCodeAt(i)
    hash = Math.imul(hash, 16777619)
  }
  return PALETTE[(hash >>> 0) % PALETTE.length]
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0].slice(0, 2).toUpperCase()
  return (parts[0][0] + parts[parts.length - 1][0]).toUpperCase()
}

function sameTarget(a: TextTarget | undefined, b: TextTarget | undefined): boolean {
  return a?.kind === b?.kind && a?.id === b?.id
}

function readPeer(clientId: number, state: Record<string, unknown>): Peer | null {
  const user = state.user as PresenceUser | undefined
  if (!user || typeof user.id !== 'string') return null
  return {
    clientId,
    user: { id: user.id, name: String(user.name ?? ''), color: String(user.color ?? colorForUser(user.id)) },
    entry: typeof state.entry === 'string' ? state.entry : null,
    selection: (state.selection as PresenceSelection | null) ?? null,
    recording: typeof state.recording === 'string' ? state.recording : null,
  }
}

const EMPTY: readonly Peer[] = Object.freeze([])

/** Wraps an Awareness with cached, per-key views so React components
 * re-render only when the peers they show change. */
export class PresenceStore {
  readonly awareness: Awareness
  private peers: readonly Peer[] = EMPTY
  private byEntry = new Map<string, readonly Peer[]>()
  private byTarget = new Map<string, readonly Peer[]>()
  private listeners = new Set<() => void>()
  private readonly onChange = () => this.rebuild()

  constructor(awareness: Awareness, user: PresenceUser) {
    this.awareness = awareness
    awareness.setLocalState({ user, entry: null, selection: null, recording: null } satisfies PresenceState)
    awareness.on('change', this.onChange)
    this.rebuild()
  }

  destroy(): void {
    this.awareness.off('change', this.onChange)
  }

  private rebuild(): void {
    const peers: Peer[] = []
    this.awareness.getStates().forEach((state, clientId) => {
      if (clientId === this.awareness.clientID) return
      const peer = readPeer(clientId, state)
      if (peer) peers.push(peer)
    })
    peers.sort((a, b) => a.user.name.localeCompare(b.user.name) || a.clientId - b.clientId)
    this.peers = samePeers(this.peers, peers) ? this.peers : peers

    const byEntry = new Map<string, Peer[]>()
    const byTarget = new Map<string, Peer[]>()
    for (const peer of peers) {
      for (const id of new Set([peer.entry, peer.recording].filter((x): x is string => x !== null))) {
        byEntry.set(id, [...(byEntry.get(id) ?? []), peer])
      }
      if (peer.selection) {
        const key = `${peer.selection.target.kind}:${peer.selection.target.id}`
        byTarget.set(key, [...(byTarget.get(key) ?? []), peer])
      }
    }
    this.byEntry = reuse(this.byEntry, byEntry)
    this.byTarget = reuse(this.byTarget, byTarget)
    for (const listener of this.listeners) listener()
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  getPeers = (): readonly Peer[] => this.peers
  peersOnEntry(id: string): readonly Peer[] {
    return this.byEntry.get(id) ?? EMPTY
  }
  peersInText(target: TextTarget): readonly Peer[] {
    return this.byTarget.get(`${target.kind}:${target.id}`) ?? EMPTY
  }

  // ---- local state -------------------------------------------------------

  private update(patch: Partial<PresenceState>): void {
    const current = this.awareness.getLocalState() as PresenceState | null
    if (!current) return
    const next = { ...current, ...patch }
    if (
      next.entry === current.entry &&
      next.recording === current.recording &&
      JSON.stringify(next.selection) === JSON.stringify(current.selection)
    ) {
      return
    }
    this.awareness.setLocalState(next)
  }

  setEntry(entry: string | null): void {
    this.update({ entry })
  }

  /** Publish the local selection inside a Y.Text. */
  setSelection(target: TextTarget, ytext: Y.Text, start: number, end: number, backward = false): void {
    const anchorIndex = backward ? end : start
    const headIndex = backward ? start : end
    this.update({
      selection: {
        target,
        anchor: Y.relativePositionToJSON(Y.createRelativePositionFromTypeIndex(ytext, anchorIndex)),
        head: Y.relativePositionToJSON(Y.createRelativePositionFromTypeIndex(ytext, headIndex)),
      },
    })
  }

  clearSelection(target?: TextTarget): void {
    const current = this.awareness.getLocalState() as PresenceState | null
    if (!current?.selection) return
    if (target && !sameTarget(current.selection.target, target)) return
    this.update({ selection: null })
  }

  /** For the recorder (audio agent): mark the entry being recorded. */
  setRecording(entry: string | null): void {
    this.update({ recording: entry })
  }
}

function samePeers(a: readonly Peer[], b: readonly Peer[]): boolean {
  return a.length === b.length && a.every((peer, i) => JSON.stringify(peer) === JSON.stringify(b[i]))
}

function reuse(previous: Map<string, readonly Peer[]>, next: Map<string, Peer[]>): Map<string, readonly Peer[]> {
  const result = new Map<string, readonly Peer[]>()
  for (const [key, peers] of next) {
    const old = previous.get(key)
    result.set(key, old && samePeers(old, peers) ? old : peers)
  }
  return result
}

/** Resolve a peer's selection to indices in `ytext`, or null if it is not
 * in this text (any more). */
export function resolveSelection(selection: PresenceSelection, ytext: Y.Text): { anchor: number; head: number } | null {
  const doc = ytext.doc
  if (!doc) return null
  try {
    const anchor = Y.createAbsolutePositionFromRelativePosition(Y.createRelativePositionFromJSON(selection.anchor), doc)
    const head = Y.createAbsolutePositionFromRelativePosition(Y.createRelativePositionFromJSON(selection.head), doc)
    if (!anchor || !head || anchor.type !== ytext || head.type !== ytext) return null
    return { anchor: anchor.index, head: head.index }
  } catch {
    return null
  }
}

// ---- hooks -----------------------------------------------------------------

export function usePeers(presence: PresenceStore): readonly Peer[] {
  return useSyncExternalStore(presence.subscribe, presence.getPeers)
}

export function usePeersOnEntry(presence: PresenceStore, id: string): readonly Peer[] {
  return useSyncExternalStore(presence.subscribe, useCallback(() => presence.peersOnEntry(id), [presence, id]))
}

export function usePeersInText(presence: PresenceStore, target: TextTarget): readonly Peer[] {
  const { kind, id } = target
  return useSyncExternalStore(
    presence.subscribe,
    useCallback(() => presence.peersInText({ kind, id } as TextTarget), [presence, kind, id]),
  )
}
