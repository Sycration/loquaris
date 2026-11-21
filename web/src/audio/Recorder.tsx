// The recorder dialog (FEATURES §10), opened from an entry card's speaker
// button through editorExtensions.openRecorder and mounted via editorMounts.
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from 'react'
import type { EditorServices } from '../editor/context'
import { useUndoState } from '../editor/useUndoState'
import { actionsFor, formatShortcut, shortcutFromEvent } from '../keybindings/keybindings'
import { Button } from '../ui/Button'
import { Dialog } from '../ui/Dialog'
import { Copy, Pause, Play, Redo, Trash, Undo, Upload } from '../ui/icons'
import { getLayers } from '../ui/overlay'
import { toast } from '../ui/toasts'
import { clipDuration } from '../doc/summary'
import { formatClock } from './crop'
import { getSavedDevice, listInputs, saveDevice, type InputDevice } from './devices'
import { diagnosticsText, RecorderSession, type Diagnostics } from './session'
import { Waveform } from './Waveform'
import './audio.css'

// ---- opening ---------------------------------------------------------------

type OpenRequest = { services: EditorServices; entryId: string; nonce: number } | null
let request: OpenRequest = null
let nonce = 0
const requestListeners = new Set<() => void>()

export function openRecorderFor(services: EditorServices, entryId: string): void {
  if (services.readOnly) {
    toast.info('You can’t change recordings in this project.')
    return
  }
  request = { services, entryId, nonce: ++nonce }
  for (const listener of requestListeners) listener()
}

const subscribeRequest = (listener: () => void) => {
  requestListeners.add(listener)
  return () => {
    requestListeners.delete(listener)
  }
}
const getRequest = () => request

/** Mounted once in the editor: owns the session of the open recorder. */
export function RecorderMount({ services }: { services: EditorServices }) {
  const req = useSyncExternalStore(subscribeRequest, getRequest)
  const [open, setOpen] = useState<{ session: RecorderSession; nonce: number } | null>(null)
  const [visible, setVisible] = useState(false)
  const handled = useRef(0)

  // A new request creates a session; the old one is released when idle.
  useEffect(() => {
    if (!req || req.services !== services || req.nonce === handled.current) return
    handled.current = req.nonce
    if (!services.store.getEntry(req.entryId)) return
    const session = new RecorderSession(services, req.entryId)
    setOpen((previous) => {
      previous?.session.close()
      return { session, nonce: req.nonce }
    })
    setVisible(true)
  }, [req, services])

  const session = open?.session ?? null
  const close = useCallback(() => {
    setVisible(false)
    session?.close()
  }, [session])

  // Closing while uploading keeps the session alive until it finishes.
  useEffect(() => {
    if (!session || visible) return
    const release = () => {
      if (session.getState().phase === 'idle' || session.getState().phase === 'starting') {
        session.dispose()
        setOpen((current) => (current?.session === session ? null : current))
        return true
      }
      return false
    }
    if (release()) return
    return session.subscribe(() => {
      release()
    })
  }, [session, visible])

  // Structural changes close the recorder (unless a take is in progress).
  useEffect(() => {
    if (!session || !visible) return
    const entry = session.entry
    const place = entry && `${entry.lesson}/${entry.order}`
    return services.store.subscribeStructure(() => {
      const now = session.entry
      const phase = session.getState().phase
      if (!now) {
        if (phase === 'idle') close()
        return
      }
      if (phase === 'idle' && `${now.lesson}/${now.order}` !== place) close()
    })
  }, [session, visible, services, close])

  useEffect(() => {
    if (!session) return
    session.onClosedWhileIdle = () => setVisible(false)
    return () => {
      session.onClosedWhileIdle = null
    }
  }, [session])

  useEffect(() => () => session?.dispose(), [session])

  if (!session) return null
  return <RecorderDialog services={services} session={session} open={visible} onClose={close} />
}

// ---- the dialog ------------------------------------------------------------

function useSession(session: RecorderSession) {
  return useSyncExternalStore(session.subscribe, session.getState)
}

function RecorderDialog({ services, session, open, onClose }: { services: EditorServices; session: RecorderSession; open: boolean; onClose: () => void }) {
  const state = useSession(session)
  const entry = useSyncExternalStore(
    useCallback((listener: () => void) => services.store.subscribeEntry(session.entryId, listener), [services, session]),
    () => services.store.getEntry(session.entryId),
  )
  const { canUndo, canRedo } = useUndoState(services.undo)
  const fileRef = useRef<HTMLInputElement>(null)
  const recording = state.phase === 'recording'
  const busy = state.phase !== 'idle'
  const clip = state.clip
  const range = session.clipRange()
  const hasClip = !!entry?.audio

  // Recorder shortcuts (FEATURES §14): the user's keymap, key repeat ignored
  // for play / record / stop.
  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return
      const layers = getLayers()
      if (layers.length !== 1) return
      const target = event.target as HTMLElement | null
      if (target && (target.tagName === 'INPUT' || target.tagName === 'SELECT' || target.tagName === 'TEXTAREA')) return
      const shortcut = shortcutFromEvent(event)
      if (!shortcut) return
      const action = actionsFor(services.keymap, shortcut, ['recorder'])[0]
      if (!action) return
      event.preventDefault()
      const once = action === 'recorder_play_pause' || action === 'recorder_record' || action === 'recorder_stop_playback'
      if (once && event.repeat) return
      switch (action) {
        case 'recorder_play_pause':
          session.togglePlay()
          break
        case 'recorder_record':
          if (recording) void session.stopRecording()
          else void session.record()
          break
        case 'recorder_stop_playback':
          session.stopPlayback()
          break
        case 'recorder_seek_back':
          session.seekBack()
          break
        case 'recorder_seek_forward':
          session.seekForward()
          break
        case 'recorder_seek_start':
          session.toStart()
          break
        case 'recorder_seek_end':
          session.seekToEnd()
          break
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [open, services, session, recording])

  const title = entry ? `${entry.participant === 'teacher' ? 'Teacher' : 'Student'} · ${entry.language === 'latin' ? 'Latin' : services.store.getMeta().l1Name}` : 'Recorder'
  const key = (id: string) => formatShortcut(services.keymap[id] ?? null)
  const length = entry?.audio ? formatClock(clipDuration(entry.audio, entry.trim)) : '--:--.--'

  return (
    <Dialog open={open} onClose={onClose} title="Record audio" size="lg" allowHistory className="recorder">
      <div className={`recorder__entry recorder__entry--${entry?.language ?? 'latin'}`}>
        <div className="recorder__entry-label">{title}</div>
        <div className="recorder__entry-text">{entry?.text || <span className="muted">Empty entry</span>}</div>
      </div>

      <DevicePicker disabled={recording || state.phase !== 'idle'} />

      <div className="recorder__actions">
        <Button size="sm" icon={<Undo />} disabled={!canUndo} onClick={() => services.commands.undo()}>
          Undo
        </Button>
        <Button size="sm" icon={<Redo />} disabled={!canRedo} onClick={() => services.commands.redo()}>
          Redo
        </Button>
        {!recording && (
          <>
            <Button size="sm" icon={<Upload />} disabled={busy} onClick={() => fileRef.current?.click()}>
              Import WAV…
            </Button>
            <input
              ref={fileRef}
              type="file"
              accept=".wav,audio/wav,audio/x-wav,audio/wave"
              hidden
              onChange={(event) => {
                const file = event.target.files?.[0]
                event.target.value = ''
                if (file) void session.importWav(file)
              }}
            />
          </>
        )}
        <Button size="sm" variant="danger" icon={<Trash />} disabled={busy || !hasClip} onClick={() => session.deleteClip()}>
          Delete clip
        </Button>
        <span className="recorder__length" title="Length of the cropped clip">
          {length}
        </span>
      </div>

      <Waveform
        mode={recording ? 'live' : clip ? 'clip' : 'empty'}
        live={session.livePeaks}
        tick={state.tick}
        clip={clip}
        range={range}
        cursor={state.cursor}
        disabled={busy}
        elapsed={state.elapsed}
        onSeek={(frame) => session.moveCursor(frame)}
        onCropCommit={(r, moving) => session.commitCrop(r, moving)}
      />
      {hasClip && !clip && !recording && (
        <p className="muted">{state.clipError ?? 'Loading the clip…'}</p>
      )}
      {state.error && (
        <p className="recorder__error" role="alert">
          {state.error}
        </p>
      )}

      <Diagnostics diagnostics={state.diagnostics} recording={recording} />

      <div className="recorder__transport">
        <Button onClick={onClose}>Close</Button>
        <span className="recorder__spacer" />
        <Button
          icon={<span aria-hidden>|◀</span>}
          disabled={!clip || busy}
          onClick={() => session.toStart()}
          title="Back to start"
          aria-label="Back to start"
        />
        <Button
          icon={state.playing ? <Pause /> : <Play />}
          disabled={!clip || busy}
          onClick={() => session.togglePlay()}
          title={`${state.playing ? 'Pause' : 'Play'} (${key('recorder_play_pause')})`}
        >
          {state.playing ? 'Pause' : 'Play'}
        </Button>
        <Button
          variant={recording ? 'secondary' : 'danger'}
          busy={state.phase === 'uploading' || state.phase === 'starting'}
          icon={<span className={recording ? 'recorder__stop' : 'recorder__dot'} aria-hidden />}
          onClick={() => (recording ? void session.stopRecording() : void session.record())}
          title={`${recording ? 'Stop' : 'Record'} (${key('recorder_record')})`}
        >
          {recording ? 'Stop' : 'Record'}
        </Button>
      </div>
    </Dialog>
  )
}

function Diagnostics({ diagnostics, recording }: { diagnostics: Diagnostics | null; recording: boolean }) {
  const warning = useMemo(() => {
    if (!diagnostics || !recording || diagnostics.elapsed < 1.5) return null
    if (diagnostics.callbacks === 0) return 'The stream started but has not received any audio callbacks.'
    if (diagnostics.samples > 0 && diagnostics.peakPercent === 0) return 'Audio samples are arriving, but they are silent.'
    return null
  }, [diagnostics, recording])
  if (!diagnostics) return null
  return (
    <details className="recorder__diagnostics" open={!!warning || undefined}>
      <summary>Diagnostics</summary>
      <dl>
        <dt>Callbacks</dt>
        <dd>{diagnostics.callbacks}</dd>
        <dt>Samples</dt>
        <dd>{diagnostics.samples}</dd>
        <dt>Peak</dt>
        <dd>{diagnostics.peakPercent.toFixed(1)}%</dd>
        <dt>Elapsed</dt>
        <dd>{diagnostics.elapsed.toFixed(1)} s</dd>
        <dt>Sample rate</dt>
        <dd>{diagnostics.sampleRate} Hz</dd>
        <dt>Channels</dt>
        <dd>{diagnostics.channels}</dd>
      </dl>
      {warning && <p className="recorder__error">{warning}</p>}
      <Button
        size="sm"
        icon={<Copy />}
        onClick={() => {
          navigator.clipboard.writeText(diagnosticsText(diagnostics)).then(
            () => toast.success('Copied the diagnostics.'),
            () => toast.error('Could not copy the diagnostics.'),
          )
        }}
      >
        Copy diagnostics
      </Button>
    </details>
  )
}

function DevicePicker({ disabled }: { disabled: boolean }) {
  const [devices, setDevices] = useState<InputDevice[]>([{ id: '', label: 'System default' }])
  const [selected, setSelected] = useState(getSavedDevice)
  const refresh = useCallback(() => {
    void listInputs().then(setDevices)
  }, [])
  useEffect(refresh, [refresh])
  const value = devices.some((device) => device.id === selected) ? selected : ''
  return (
    <div className="recorder__device">
      <label htmlFor="recorder-input">Input</label>
      <select
        id="recorder-input"
        className="input select"
        value={value}
        disabled={disabled}
        onChange={(event) => {
          setSelected(event.target.value)
          saveDevice(event.target.value)
        }}
      >
        {devices.map((device) => (
          <option key={device.id} value={device.id}>
            {device.label}
          </option>
        ))}
      </select>
      <Button size="sm" disabled={disabled} onClick={refresh}>
        Refresh
      </Button>
    </div>
  )
}
