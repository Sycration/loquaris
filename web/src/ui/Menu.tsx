// Popup menus: context menus at a point and dropdowns from a button. WAI-ARIA
// menu pattern: arrow keys, Home/End, type-ahead by first letter, Enter or
// Space activates, Escape or Tab closes, a click outside closes.
import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode, type RefObject } from 'react'
import { createPortal } from 'react-dom'
import { IconButton } from './Button'
import { isTopLayer, useOverlayLayer } from './overlay'

export type MenuItem =
  | {
      kind?: 'item'
      label: string
      onSelect: () => void
      disabled?: boolean
      danger?: boolean
      /** Shortcut hint shown at the right, e.g. "Ctrl+D". */
      shortcut?: string
      title?: string
    }
  | { kind: 'separator' }

export interface MenuPosition {
  x: number
  y: number
}

export interface MenuProps {
  items: MenuItem[]
  /** Viewport coordinates of the menu's top-left corner (before clamping). */
  position: MenuPosition
  onClose: () => void
  label: string
  /** Element that opened the menu; focus returns there on close. */
  returnFocus?: RefObject<HTMLElement | null>
}

export function Menu({ items, position, onClose, label, returnFocus }: MenuProps) {
  const layerId = useOverlayLayer(true, 'menu')
  const ref = useRef<HTMLDivElement>(null)
  const [placed, setPlaced] = useState<MenuPosition | null>(null)
  const onCloseRef = useRef(onClose)
  useLayoutEffect(() => {
    onCloseRef.current = onClose
  })

  useLayoutEffect(() => {
    const element = ref.current
    if (!element) return
    const { width, height } = element.getBoundingClientRect()
    const margin = 8
    setPlaced({
      x: Math.max(margin, Math.min(position.x, window.innerWidth - width - margin)),
      y: Math.max(margin, Math.min(position.y, window.innerHeight - height - margin)),
    })
    element.querySelector<HTMLElement>('[role="menuitem"]:not([aria-disabled="true"])')?.focus()
  }, [position.x, position.y])

  useEffect(() => {
    const close = () => onCloseRef.current()
    const onPointerDown = (event: PointerEvent) => {
      if (ref.current && !ref.current.contains(event.target as Node)) close()
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isTopLayer(layerId)) return
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopPropagation()
        close()
        returnFocus?.current?.focus()
      }
    }
    document.addEventListener('pointerdown', onPointerDown, true)
    document.addEventListener('keydown', onKeyDown, true)
    window.addEventListener('blur', close)
    window.addEventListener('resize', close)
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true)
      document.removeEventListener('keydown', onKeyDown, true)
      window.removeEventListener('blur', close)
      window.removeEventListener('resize', close)
    }
  }, [layerId, returnFocus])

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const menuItems = Array.from(ref.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? []).filter(
      (item) => item.getAttribute('aria-disabled') !== 'true',
    )
    const index = menuItems.indexOf(document.activeElement as HTMLElement)
    const focus = (i: number) => menuItems[(i + menuItems.length) % menuItems.length]?.focus()
    if (event.key === 'ArrowDown') focus(index + 1)
    else if (event.key === 'ArrowUp') focus(index - 1)
    else if (event.key === 'Home') focus(0)
    else if (event.key === 'End') focus(menuItems.length - 1)
    else if (event.key === 'Tab') {
      onClose()
      return
    } else if (event.key.length === 1 && /\S/.test(event.key)) {
      const letter = event.key.toLowerCase()
      const start = index + 1
      for (let i = 0; i < menuItems.length; i++) {
        const item = menuItems[(start + i) % menuItems.length]
        if (item.textContent?.trim().toLowerCase().startsWith(letter)) {
          item.focus()
          break
        }
      }
    } else return
    event.preventDefault()
    event.stopPropagation()
  }

  return createPortal(
    <div
      ref={ref}
      className="menu"
      role="menu"
      aria-label={label}
      style={{ left: (placed ?? position).x, top: (placed ?? position).y, visibility: placed ? 'visible' : 'hidden' }}
      onKeyDown={onKeyDown}
      onContextMenu={(event) => event.preventDefault()}
    >
      {items.map((item, i) =>
        item.kind === 'separator' ? (
          <div key={`sep-${i}`} className="menu__separator" role="separator" />
        ) : (
          <button
            key={`${item.label}-${i}`}
            type="button"
            role="menuitem"
            tabIndex={-1}
            className={['menu__item', item.danger && 'menu__item--danger'].filter(Boolean).join(' ')}
            aria-disabled={item.disabled || undefined}
            title={item.title}
            onClick={() => {
              if (item.disabled) return
              onClose()
              returnFocus?.current?.focus()
              item.onSelect()
            }}
          >
            <span className="menu__label">{item.label}</span>
            {item.shortcut && <kbd className="menu__shortcut">{item.shortcut}</kbd>}
          </button>
        ),
      )}
    </div>,
    document.body,
  )
}

export interface MenuButtonProps {
  label: string
  items: MenuItem[] | (() => MenuItem[])
  children: ReactNode
  size?: 'sm' | 'md'
  className?: string
  align?: 'start' | 'end'
  disabled?: boolean
}

/** An icon button that opens a dropdown menu below it. */
export function MenuButton({ label, items, children, size, className, align = 'end', disabled }: MenuButtonProps) {
  const [open, setOpen] = useState<{ position: MenuPosition; items: MenuItem[] } | null>(null)
  const buttonRef = useRef<HTMLButtonElement>(null)
  const show = () => {
    const rect = buttonRef.current!.getBoundingClientRect()
    const list = typeof items === 'function' ? items() : items
    // Estimate the width for end alignment; the menu clamps itself anyway.
    const width = 220
    setOpen({ position: { x: align === 'end' ? rect.right - width : rect.left, y: rect.bottom + 4 }, items: list })
  }
  return (
    <>
      <IconButton
        ref={buttonRef}
        label={label}
        size={size}
        className={className}
        aria-haspopup="menu"
        aria-expanded={open !== null}
        disabled={disabled}
        onClick={() => (open ? setOpen(null) : show())}
        onKeyDown={(event) => {
          if (event.key === 'ArrowDown') {
            event.preventDefault()
            show()
          }
        }}
      >
        {children}
      </IconButton>
      {open && (
        <Menu items={open.items} position={open.position} label={label} onClose={() => setOpen(null)} returnFocus={buttonRef} />
      )}
    </>
  )
}
