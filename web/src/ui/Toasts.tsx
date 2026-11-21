import type { ReactNode } from 'react'
import { Check, Close, Copy, Info, Warning } from './icons'
import { toast as defaultStore, useToasts, type ToastLevel, type ToastStore } from './toasts'

const ICONS: Record<ToastLevel, ReactNode> = {
  info: <Info />,
  success: <Check />,
  error: <Warning />,
}

/** The bottom-right toast stack. Mounted once by App. */
export function ToastViewport({ store = defaultStore }: { store?: ToastStore }) {
  const toasts = useToasts(store)
  return (
    <section
      className="toasts"
      aria-label="Notifications"
      onPointerEnter={() => store.setHovered(true)}
      onPointerLeave={() => store.setHovered(false)}
    >
      {toasts.map((item) => (
        <div
          key={item.id}
          className={`toast toast--${item.level}`}
          role={item.level === 'error' ? 'alert' : 'status'}
          aria-live={item.level === 'error' ? 'assertive' : 'polite'}
        >
          <span className="toast__icon" aria-hidden>
            {ICONS[item.level]}
          </span>
          <p className="toast__text">{item.text}</p>
          <div className="toast__actions">
            {item.count > 1 && (
              <span className="toast__count" aria-label={`Shown ${item.count} times`}>
                ×{item.count}
              </span>
            )}
            <button
              type="button"
              className="icon-btn icon-btn--ghost icon-btn--sm"
              aria-label="Copy message"
              title="Copy"
              onClick={() => void navigator.clipboard?.writeText(item.text)}
            >
              <Copy />
            </button>
            <button
              type="button"
              className="icon-btn icon-btn--ghost icon-btn--sm"
              aria-label="Dismiss"
              title="Dismiss"
              onClick={() => store.dismiss(item.id)}
            >
              <Close />
            </button>
          </div>
        </div>
      ))}
    </section>
  )
}
