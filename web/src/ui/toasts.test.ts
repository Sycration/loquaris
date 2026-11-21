import { afterEach, describe, expect, it, vi } from 'vitest'
import { MAX_TOASTS, TOAST_LIFETIME_MS, ToastStore } from './toasts'

afterEach(() => vi.useRealTimers())

describe('ToastStore', () => {
  it('counts repeats, moving them to the bottom', () => {
    const store = new ToastStore()
    store.error('Disk full')
    store.success('Saved')
    store.error('Disk full')
    expect(store.getToasts().map((t) => [t.level, t.text, t.count])).toEqual([
      ['success', 'Saved', 1],
      ['error', 'Disk full', 2],
    ])
  })

  it('expires info and success but keeps errors', () => {
    let now = 0
    const store = new ToastStore(() => now)
    store.error('Broken')
    store.info('Hello')
    now = TOAST_LIFETIME_MS + 1
    store.expire()
    expect(store.getToasts().map((t) => t.text)).toEqual(['Broken'])
  })

  it('pauses expiry while hovered', () => {
    let now = 0
    const store = new ToastStore(() => now)
    store.info('Hello')
    store.setHovered(true)
    now = TOAST_LIFETIME_MS * 2
    store.expire()
    expect(store.getToasts().length).toBe(1)
    store.setHovered(false)
    now += TOAST_LIFETIME_MS - 100
    store.expire()
    expect(store.getToasts().length).toBe(1)
  })

  it('drops the oldest beyond the limit', () => {
    const store = new ToastStore()
    for (let i = 0; i <= MAX_TOASTS; i++) store.error(String(i))
    expect(store.getToasts().length).toBe(MAX_TOASTS)
    expect(store.getToasts()[0].text).toBe('1')
  })

  it('expires on a timer', () => {
    vi.useFakeTimers()
    const store = new ToastStore()
    store.success('Done')
    vi.advanceTimersByTime(TOAST_LIFETIME_MS + 100)
    expect(store.getToasts().length).toBe(0)
  })
})
