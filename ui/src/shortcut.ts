// Peek key: Tauri accelerator strings ("Alt+Shift+Space") ↔ mac glyphs (⌥⇧Space), and
// recording one from a keydown. Pure.
import { t } from './i18n'

export const DEFAULT_PEEK = 'Alt+Shift+Space'

type Mod = 'Ctrl' | 'Alt' | 'Shift' | 'Cmd'
const ORDER: Mod[] = ['Ctrl', 'Alt', 'Shift', 'Cmd'] // mac display order ⌃⌥⇧⌘
const GLYPH: Record<Mod, string> = { Ctrl: '⌃', Alt: '⌥', Shift: '⇧', Cmd: '⌘' }
const ALIASES: Record<string, Mod> = {
  ctrl: 'Ctrl', control: 'Ctrl',
  alt: 'Alt', option: 'Alt',
  shift: 'Shift',
  cmd: 'Cmd', command: 'Cmd', super: 'Cmd', meta: 'Cmd',
  commandorcontrol: 'Cmd', cmdorctrl: 'Cmd', commandorctrl: 'Cmd', cmdorcontrol: 'Cmd', // mac: ⌘
}
const KEY_GLYPH: Record<string, string> = { Up: '↑', Down: '↓', Left: '←', Right: '→' }

export function displayAccelerator(accel: string): string {
  const mods = new Set<Mod>()
  let key = ''
  for (const part of accel.split('+')) {
    const m = ALIASES[part.toLowerCase()]
    if (m) mods.add(m)
    else key = part.length === 1 ? part.toUpperCase() : part
  }
  return ORDER.filter((m) => mods.has(m)).map((m) => GLYPH[m]).join('') + (KEY_GLYPH[key] ?? key)
}

export type KeyEventLike = Pick<KeyboardEvent, 'code' | 'key' | 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>
export type Recorded =
  | { kind: 'ok'; accelerator: string }
  | { kind: 'wait' } // only modifiers so far
  | { kind: 'cancel' }
  | { kind: 'invalid'; reason: string }

const NAMED: Record<string, string> = {
  Space: 'Space', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace',
  ArrowUp: 'Up', ArrowDown: 'Down', ArrowLeft: 'Left', ArrowRight: 'Right',
  Minus: 'Minus', Equal: 'Equal', Comma: 'Comma', Period: 'Period', Slash: 'Slash',
  Semicolon: 'Semicolon', Quote: 'Quote', Backquote: 'Backquote', Backslash: 'Backslash',
  BracketLeft: 'BracketLeft', BracketRight: 'BracketRight',
}
const MODIFIER_CODES = /^(Meta|OS|Control|Alt|Shift)(Left|Right)?$|^CapsLock$|^Fn$/

/** Physical key (e.code) → accelerator key name, layout-independent. */
function keyName(code: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit[0-9]$/.test(code)) return code.slice(5)
  if (/^F([1-9]|1[0-9]|20)$/.test(code)) return code
  return NAMED[code] ?? null
}

export function recordShortcut(e: KeyEventLike): Recorded {
  if (e.code === 'Escape') return { kind: 'cancel' }
  if (MODIFIER_CODES.test(e.code)) return { kind: 'wait' }
  if (!e.metaKey && !e.ctrlKey && !e.altKey) return { kind: 'invalid', reason: t('rec_need_mod') }
  const key = keyName(e.code)
  if (!key) return { kind: 'invalid', reason: t('rec_no_name') }
  const mods: Mod[] = []
  if (e.ctrlKey) mods.push('Ctrl')
  if (e.altKey) mods.push('Alt')
  if (e.shiftKey) mods.push('Shift')
  if (e.metaKey) mods.push('Cmd')
  return { kind: 'ok', accelerator: [...mods, key].join('+') }
}
