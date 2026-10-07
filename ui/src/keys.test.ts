import { describe, expect, it } from 'vitest'
import { t } from './i18n'
import { capOf, KEYMAP, keyAction, keysList, type KeyCtx, type KeyIn } from './keys'

const k = (key: string, p: Partial<KeyIn> = {}): KeyIn => ({ key, meta: false, ctrl: false, alt: false, shift: false, target: 'none', ...p })
const collapsed: KeyCtx = { mode: 'collapsed', pane: 'main', peeking: false }
const expanded: KeyCtx = { mode: 'expanded', pane: 'main', peeking: false }
const settings: KeyCtx = { mode: 'expanded', pane: 'settings', peeking: false }

describe('keyAction', () => {
  it('space starts/stops', () => {
    expect(keyAction(k(' '), collapsed)).toBe('track')
    expect(keyAction(k(' '), expanded)).toBe('track')
  })
  it('↓ expands, ↑ collapses, enter toggles', () => {
    expect(keyAction(k('ArrowDown'), collapsed)).toBe('expand')
    expect(keyAction(k('ArrowUp'), expanded)).toBe('collapse')
    expect(keyAction(k('ArrowDown'), expanded)).toBeNull()
    expect(keyAction(k('Enter'), collapsed)).toBe('expand')
    expect(keyAction(k('Enter'), expanded)).toBe('collapse')
  })
  it('esc steps back: settings → panel → pill → hidden', () => {
    expect(keyAction(k('Escape'), settings)).toBe('back')
    expect(keyAction(k('Escape'), expanded)).toBe('collapse')
    expect(keyAction(k('Escape'), collapsed)).toBe('hide')
  })
    it('esc during a peek steps down to the pill (the backend ends the peek)', () => {
    expect(keyAction(k('Escape'), { ...expanded, peeking: true })).toBe('end-peek')
    expect(keyAction(k('Escape'), { ...settings, peeking: true })).toBe('back')
  })
  it('esc, esc after a peek: pill first, then hidden — never straight to hidden', () => {
    // 1st esc while peeking → end-peek; the backend answers with {open:false, mode:'pill'},
    // so the 2nd esc sees a collapsed, non-peeking window and hides it.
    expect(keyAction(k('Escape'), { ...expanded, peeking: true })).toBe('end-peek')
    expect(keyAction(k('Escape'), { ...collapsed, peeking: false })).toBe('hide')
    // A peek never leaves esc mapped to hide while still expanded.
    expect(keyAction(k('Escape'), { ...expanded, peeking: true })).not.toBe('hide')
  })
  it('⌘ shortcuts', () => {
    expect(keyAction(k('w', { meta: true }), expanded)).toBe('hide')
    expect(keyAction(k('e', { meta: true }), collapsed)).toBe('export')
    expect(keyAction(k('E', { meta: true, shift: true }), collapsed)).toBe('export')
    expect(keyAction(k(',', { meta: true }), collapsed)).toBe('settings')
    expect(keyAction(k('q', { meta: true }), expanded)).toBe('quit')
  })
  it('? shows the keys list', () => {
    expect(keyAction(k('?', { shift: true }), collapsed)).toBe('keys')
  })
  it('1 / 2 / 3 pick the range tab while the panel shows it', () => {
    expect(keyAction(k('1'), expanded)).toBe('range-session')
    expect(keyAction(k('2'), expanded)).toBe('range-today')
    expect(keyAction(k('3'), expanded)).toBe('range-week')
    expect(keyAction(k('1'), collapsed)).toBeNull()
    expect(keyAction(k('1'), settings)).toBeNull()
  })
  it('typing in a field only honours ⌘ shortcuts and esc', () => {
    const t = { target: 'field' as const }
    expect(keyAction(k(' ', t), settings)).toBeNull()
    expect(keyAction(k('1', t), settings)).toBeNull()
    expect(keyAction(k('Enter', t), settings)).toBeNull()
    expect(keyAction(k('Escape', t), settings)).toBe('back')
    expect(keyAction(k(',', { ...t, meta: true }), settings)).toBe('settings')
  })
  it('space / enter on a keyboard-focused button press that button instead', () => {
    expect(keyAction(k(' ', { target: 'button' }), expanded)).toBeNull()
    expect(keyAction(k('Enter', { target: 'button' }), expanded)).toBeNull()
  })
  it('ignores other modifier combos and unknown keys', () => {
    expect(keyAction(k(' ', { ctrl: true }), expanded)).toBeNull()
    expect(keyAction(k('x'), expanded)).toBeNull()
  })
})

describe('keys list (settings) comes from the same table', () => {
  it('every bound key appears among its row\'s caps', () => {
    for (const row of KEYMAP) {
      const caps = row.caps.split(' ')
      for (const b of row.binds) expect(caps).toContain(capOf(b))
    }
  })
  it('⌘W says how to get the window back (no × in the pill any more)', () => {
    expect(keysList('⌥⇧Space').find(([c]) => c === '⌘W')?.[1]).toBe('Hide · back with the peek key')
  })
  it('lists every row, in order, as [caps, description], then the global peek key', () => {
    const list = keysList('⌥⇧Space')
    expect(list.slice(0, -1)).toEqual(KEYMAP.map((r) => [r.caps, t(r.desc)]))
    expect(list.map(([c]) => c)).toEqual(['space', '↓ ↑ ↵', 'esc', '⌘W', '1 2 3', '⌘E', '⌘,', '⌘Q', '?', '⌥⇧Space'])
    expect(list[list.length - 1]).toEqual(['⌥⇧Space', 'Peek (global)'])
  })
})
