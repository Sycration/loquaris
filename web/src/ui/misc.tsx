import { useState, type CSSProperties, type ReactNode } from 'react'
import { colorForUser, initials } from '../collab/presence'
import { Button } from './Button'
import { Check, Copy } from './icons'
import { toast } from './toasts'
import type { Role } from '../api/types'

export function Spinner({ label = 'Loading…' }: { label?: string }) {
  return (
    <span className="spinner-wrap" role="status">
      <span className="spinner" aria-hidden />
      <span className="visually-hidden">{label}</span>
    </span>
  )
}

/** A centred loading state for whole pages. */
export function PageLoading({ label = 'Loading…' }: { label?: string }) {
  return (
    <div className="page-loading">
      <Spinner label={label} />
    </div>
  )
}

const ROLE_LABEL: Record<Role, string> = { owner: 'Owner', editor: 'Editor', viewer: 'Viewer' }

export function roleLabel(role: Role): string {
  return ROLE_LABEL[role]
}

export function RoleBadge({ role }: { role: Role }) {
  return <span className={`badge badge--${role}`}>{ROLE_LABEL[role]}</span>
}

export function Badge({ children, tone = 'neutral' }: { children: ReactNode; tone?: 'neutral' | 'accent' | 'danger' | 'success' }) {
  return <span className={`badge badge--${tone}`}>{children}</span>
}

export function Avatar({ name, userId, color, size = 'md', title }: { name: string; userId: string; color?: string; size?: 'sm' | 'md'; title?: string }) {
  return (
    <span
      className={`avatar avatar--${size}`}
      style={{ '--avatar-color': color ?? colorForUser(userId) } as CSSProperties}
      title={title ?? name}
      aria-label={title ?? name}
      role="img"
    >
      {initials(name)}
    </span>
  )
}

export function EmptyState({ title, children, action }: { title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty-state">
      <h3>{title}</h3>
      {children && <div className="empty-state__body">{children}</div>}
      {action && <div className="empty-state__action">{action}</div>}
    </div>
  )
}

export async function copyText(text: string, message = 'Copied to the clipboard.'): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text)
    toast.success(message)
    return true
  } catch {
    toast.error('Could not copy to the clipboard. Select the text and copy it manually.')
    return false
  }
}

/** A read-only value (link, token) with a Copy button. */
export function CopyField({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false)
  return (
    <div className="copy-field">
      <input className="input copy-field__input" readOnly value={value} aria-label={label} onFocus={(event) => event.currentTarget.select()} />
      <Button
        icon={copied ? <Check /> : <Copy />}
        onClick={async () => {
          if (await copyText(value, 'Link copied.')) {
            setCopied(true)
            setTimeout(() => setCopied(false), 1500)
          }
        }}
      >
        {copied ? 'Copied' : 'Copy'}
      </Button>
    </div>
  )
}

const UNITS: [Intl.RelativeTimeFormatUnit, number][] = [
  ['year', 365 * 24 * 3600],
  ['month', 30 * 24 * 3600],
  ['week', 7 * 24 * 3600],
  ['day', 24 * 3600],
  ['hour', 3600],
  ['minute', 60],
]

const relativeFormat = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' })

/** "3 minutes ago", "yesterday", "just now". */
export function relativeTime(iso: string, now = Date.now()): string {
  const time = Date.parse(iso)
  if (Number.isNaN(time)) return ''
  const seconds = (time - now) / 1000
  for (const [unit, size] of UNITS) {
    if (Math.abs(seconds) >= size) return relativeFormat.format(Math.round(seconds / size), unit)
  }
  return 'just now'
}

export function formatDate(iso: string): string {
  const time = Date.parse(iso)
  if (Number.isNaN(time)) return ''
  return new Date(time).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' })
}

export function Time({ iso }: { iso: string }) {
  return (
    <time dateTime={iso} title={formatDate(iso)}>
      {relativeTime(iso)}
    </time>
  )
}
