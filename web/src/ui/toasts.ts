// One global toast stack (FEATURES §13, DESIGN.md: "there is one toast
// stack"). Info and success expire after 4 s, counted while not hovered;
// errors stay until dismissed. At most 5; a repeat of a visible message moves
// to the bottom with a count.
import { useSyncExternalStore } from 'react'

export type ToastLevel = 'info' | 'success' | 'error'

export interface Toast {
  id: number
  level: ToastLevel
  text: string
  count: number
  /** When the message was raised or last hovered (ms); it expires from then. */
  since: number
}

export const TOAST_LIFETIME_MS = 4000
export const MAX_TOASTS = 5

type Listener = () => void

export class ToastStore {
  private toasts: readonly Toast[] = []
  private nextId = 1
  private listeners = new Set<Listener>()
  private timer: ReturnType<typeof setTimeout> | null = null
  private hovered = false
  private now: () => number

  constructor(now: () => number = () => Date.now()) {
    this.now = now
  }

  info = (text: string) => this.push('info', text)
  success = (text: string) => this.push('success', text)
  error = (text: string) => this.push('error', text)

  push(level: ToastLevel, text: string): void {
    const now = this.now()
    const list = [...this.toasts]
    const index = list.findIndex((toast) => toast.level === level && toast.text === text)
    let toast: Toast
    if (index >= 0) {
      toast = { ...list[index], count: list[index].count + 1, since: now }
      list.splice(index, 1)
    } else {
      toast = { id: this.nextId++, level, text, count: 1, since: now }
    }
    list.push(toast)
    while (list.length > MAX_TOASTS) list.shift()
    this.set(list)
  }

  dismiss(id: number): void {
    this.set(this.toasts.filter((toast) => toast.id !== id))
  }

  clear(): void {
    this.set([])
  }

  /** While the pointer is over the stack, nothing expires. */
  setHovered(hovered: boolean): void {
    this.hovered = hovered
    if (!hovered) {
      const now = this.now()
      this.set(this.toasts.map((toast) => ({ ...toast, since: now })))
    } else {
      this.schedule()
    }
  }

  /** Drop expired messages. Public for tests. */
  expire(): void {
    if (this.hovered) return
    const now = this.now()
    const kept = this.toasts.filter((toast) => toast.level === 'error' || now - toast.since < TOAST_LIFETIME_MS)
    if (kept.length !== this.toasts.length) this.set(kept)
    else this.schedule()
  }

  private set(toasts: readonly Toast[]): void {
    this.toasts = toasts
    this.schedule()
    for (const listener of this.listeners) listener()
  }

  private schedule(): void {
    if (this.timer) clearTimeout(this.timer)
    this.timer = null
    if (this.hovered) return
    const now = this.now()
    const remaining = this.toasts
      .filter((toast) => toast.level !== 'error')
      .map((toast) => TOAST_LIFETIME_MS - (now - toast.since))
    if (remaining.length === 0) return
    this.timer = setTimeout(() => this.expire(), Math.max(0, Math.min(...remaining)) + 20)
  }

  getToasts = (): readonly Toast[] => this.toasts

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }
}

/** The app's toast stack. */
export const toast = new ToastStore()

export function useToasts(store: ToastStore = toast): readonly Toast[] {
  return useSyncExternalStore(store.subscribe, store.getToasts)
}

/** A message for an unknown error value. */
export function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error)
}
