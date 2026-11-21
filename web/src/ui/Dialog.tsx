// Modal dialogs: rendered in a portal, focus moved in and trapped, Escape
// closes the topmost one, focus returns to where it was on close. The rest
// of the page is made inert while open.
import { useEffect, useId, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { Button } from './Button'
import { Close } from './icons'
import { isTopLayer, useOverlayLayer } from './overlay'

const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), [tabindex]:not([tabindex="-1"]), [contenteditable="true"]'

export function focusableWithin(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) => !element.hasAttribute('inert') && element.getClientRects().length > 0,
  )
}

export interface DialogProps {
  open: boolean
  onClose: () => void
  title: ReactNode
  description?: ReactNode
  children?: ReactNode
  footer?: ReactNode
  size?: 'sm' | 'md' | 'lg'
  /** Element to focus first; defaults to the first field or button. */
  initialFocus?: RefObject<HTMLElement | null>
  /** Let undo/redo shortcuts reach the document (dialogs that edit it). */
  allowHistory?: boolean
  className?: string
}

export function Dialog({ open, onClose, title, description, children, footer, size = 'md', initialFocus, allowHistory, className }: DialogProps) {
  const layerId = useOverlayLayer(open, 'dialog', allowHistory)
  const panelRef = useRef<HTMLDivElement>(null)
  const titleId = useId()
  const descriptionId = useId()
  const onCloseRef = useRef(onClose)
  useLayoutEffect(() => {
    onCloseRef.current = onClose
  })

  useEffect(() => {
    if (!open) return
    const previous = document.activeElement as HTMLElement | null
    const panel = panelRef.current
    if (panel) {
      const target =
        initialFocus?.current ??
        panel.querySelector<HTMLElement>('[data-autofocus]') ??
        panel.querySelector<HTMLElement>('input:not([disabled]), textarea:not([disabled]), select:not([disabled])') ??
        focusableWithin(panel).find((element) => !element.classList.contains('dialog__close')) ??
        panel
      target.focus()
    }
    return () => {
      if (previous && document.contains(previous)) previous.focus()
    }
    // Only when opening.
  }, [open])

  useEffect(() => {
    if (!open) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isTopLayer(layerId)) return
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopPropagation()
        onCloseRef.current()
      } else if (event.key === 'Tab' && panelRef.current) {
        const items = focusableWithin(panelRef.current)
        if (items.length === 0) {
          event.preventDefault()
          return
        }
        const first = items[0]
        const last = items[items.length - 1]
        const active = document.activeElement
        if (event.shiftKey && (active === first || !panelRef.current.contains(active))) {
          event.preventDefault()
          last.focus()
        } else if (!event.shiftKey && (active === last || !panelRef.current.contains(active))) {
          event.preventDefault()
          first.focus()
        }
      }
    }
    document.addEventListener('keydown', onKeyDown, true)
    return () => document.removeEventListener('keydown', onKeyDown, true)
  }, [open, layerId])

  if (!open) return null
  return createPortal(
    <div
      className="dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose()
      }}
    >
      <div
        ref={panelRef}
        className={['dialog', `dialog--${size}`, className].filter(Boolean).join(' ')}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={description ? descriptionId : undefined}
        tabIndex={-1}
      >
        <header className="dialog__header">
          <h2 className="dialog__title" id={titleId}>
            {title}
          </h2>
          <button type="button" className="icon-btn icon-btn--ghost icon-btn--sm dialog__close" aria-label="Close" title="Close" onClick={onClose}>
            <Close />
          </button>
        </header>
        {description && (
          <p className="dialog__description" id={descriptionId}>
            {description}
          </p>
        )}
        {children && <div className="dialog__body">{children}</div>}
        {footer && <footer className="dialog__footer">{footer}</footer>}
      </div>
    </div>,
    document.body,
  )
}

// ---- confirm() --------------------------------------------------------------

export interface ConfirmOptions {
  title: string
  message?: ReactNode
  confirmLabel?: string
  cancelLabel?: string
  danger?: boolean
}

interface PendingConfirm extends ConfirmOptions {
  resolve: (ok: boolean) => void
}

let pending: PendingConfirm | null = null
const confirmListeners = new Set<() => void>()

/** Ask a yes/no question in a dialog. Resolves false on cancel. Requires
 * <ConfirmHost/> to be mounted (App does this). */
export function confirm(options: ConfirmOptions): Promise<boolean> {
  return new Promise((resolve) => {
    pending?.resolve(false)
    pending = { ...options, resolve }
    for (const listener of confirmListeners) listener()
  })
}

function settle(ok: boolean) {
  const current = pending
  pending = null
  current?.resolve(ok)
  for (const listener of confirmListeners) listener()
}

export function ConfirmHost() {
  const current = useSyncExternalStore(
    (listener) => {
      confirmListeners.add(listener)
      return () => confirmListeners.delete(listener)
    },
    () => pending,
  )
  const confirmRef = useRef<HTMLButtonElement>(null)
  const [snapshot, setSnapshot] = useState<PendingConfirm | null>(null)
  if (current && current !== snapshot) setSnapshot(current)
  const shown = current ?? snapshot
  return (
    <Dialog
      open={current !== null}
      onClose={() => settle(false)}
      title={shown?.title ?? ''}
      size="sm"
      initialFocus={confirmRef}
      footer={
        <>
          <Button variant="ghost" onClick={() => settle(false)}>
            {shown?.cancelLabel ?? 'Cancel'}
          </Button>
          <Button ref={confirmRef} variant={shown?.danger ? 'danger' : 'primary'} onClick={() => settle(true)}>
            {shown?.confirmLabel ?? 'OK'}
          </Button>
        </>
      }
    >
      {shown?.message && <div className="confirm__message">{shown.message}</div>}
    </Dialog>
  )
}
