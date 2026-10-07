import { describe, expect, it } from 'vitest'
import { DEFAULT_PEEK, displayAccelerator, recordShortcut, type KeyEventLike } from './shortcut'

const ev = (code: string, mods: Partial<KeyEventLike> = {}): KeyEventLike => ({
  code, key: '', metaKey: false, ctrlKey: false, altKey: false, shiftKey: false, ...mods,
})

describe('displayAccelerator (Tauri accelerator → mac glyphs)', () => {
  it('shows the default peek key', () => {
    expect(DEFAULT_PEEK).toBe('Alt+Shift+Space')
    expect(displayAccelerator('Alt+Shift+Space')).toBe('⌥⇧Space')
  })
  it('orders modifiers the mac way: ⌃⌥⇧⌘', () => {
    expect(displayAccelerator('Cmd+Shift+Ctrl+Alt+K')).toBe('⌃⌥⇧⌘K')
  })
  it('accepts the usual aliases, any case', () => {
    expect(displayAccelerator('CommandOrControl+option+t')).toBe('⌥⌘T')
    expect(displayAccelerator('control+super+1')).toBe('⌃⌘1')
  })
  it('uses arrows for arrow keys', () => {
    expect(displayAccelerator('Ctrl+Alt+Up')).toBe('⌃⌥↑')
  })
})

describe('recordShortcut (keydown → Tauri accelerator)', () => {
  it('records modifiers + key in canonical order', () => {
    expect(recordShortcut(ev('Space', { altKey: true, shiftKey: true }))).toEqual({ kind: 'ok', accelerator: 'Alt+Shift+Space' })
    expect(recordShortcut(ev('KeyK', { metaKey: true, ctrlKey: true }))).toEqual({ kind: 'ok', accelerator: 'Ctrl+Cmd+K' })
    expect(recordShortcut(ev('Digit2', { altKey: true }))).toEqual({ kind: 'ok', accelerator: 'Alt+2' })
    expect(recordShortcut(ev('ArrowUp', { ctrlKey: true, altKey: true }))).toEqual({ kind: 'ok', accelerator: 'Ctrl+Alt+Up' })
    expect(recordShortcut(ev('F5', { metaKey: true }))).toEqual({ kind: 'ok', accelerator: 'Cmd+F5' })
  })
  it('roundtrips through the display form', () => {
    const r = recordShortcut(ev('KeyP', { altKey: true, shiftKey: true, metaKey: true }))
    expect(r.kind === 'ok' && displayAccelerator(r.accelerator)).toBe('⌥⇧⌘P')
  })
  it('esc cancels', () => {
    expect(recordShortcut(ev('Escape'))).toEqual({ kind: 'cancel' })
  })
  it('modifiers alone: keep waiting', () => {
    expect(recordShortcut(ev('AltLeft', { altKey: true }))).toEqual({ kind: 'wait' })
    expect(recordShortcut(ev('ShiftRight', { shiftKey: true, altKey: true }))).toEqual({ kind: 'wait' })
  })
  it('needs at least one of ⌘⌃⌥ (shift alone is not enough)', () => {
    expect(recordShortcut(ev('KeyA')).kind).toBe('invalid')
    expect(recordShortcut(ev('KeyA', { shiftKey: true })).kind).toBe('invalid')
  })
  it('rejects keys it cannot name', () => {
    expect(recordShortcut(ev('IntlRo', { altKey: true })).kind).toBe('invalid')
  })
})
