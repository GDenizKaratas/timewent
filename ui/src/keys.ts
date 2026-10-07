// Keyboard map: one table drives both the dispatcher and the list shown in settings.
import { t, type Key } from './i18n'

export type Action =
  | 'track' | 'expand' | 'collapse' | 'back' | 'hide' | 'export' | 'settings' | 'quit' | 'keys'
  | 'range-session' | 'range-today' | 'range-week' | 'end-peek'

export type KeyCtx = { mode: 'collapsed' | 'expanded'; pane: 'main' | 'settings'; peeking: boolean }
export type KeyIn = {
  key: string
  meta: boolean
  ctrl: boolean
  alt: boolean
  shift: boolean
  target: 'none' | 'field' | 'button' // what has keyboard focus
}

type Binding = { key: string; meta?: boolean; act: (c: KeyCtx) => Action | null }
export type KeyRow = { caps: string; desc: Key; binds: Binding[] }

const always = (a: Action) => () => a
const inPanel = (a: Action) => (c: KeyCtx) => (c.mode === 'expanded' && c.pane === 'main' ? a : null)

export const KEYMAP: KeyRow[] = [
  { caps: 'space', desc: 'key.track', binds: [{ key: ' ', act: always('track') }] },
  {
    caps: '↓ ↑ ↵',
    desc: 'key.expand',
    binds: [
      { key: 'ArrowDown', act: (c) => (c.mode === 'collapsed' ? 'expand' : null) },
      { key: 'ArrowUp', act: (c) => (c.mode === 'expanded' ? 'collapse' : null) },
      { key: 'Enter', act: (c) => (c.mode === 'collapsed' ? 'expand' : 'collapse') },
    ],
  },
  {
    caps: 'esc',
    desc: 'key.esc',
    binds: [
      {
        key: 'Escape',
                // During a peek, esc steps down to the pill: the backend ends the peek and replies with
        // {open:false, mode:'pill'}; the next esc hides (and the backend hands focus back).
        act: (c) => (c.pane === 'settings' ? 'back' : c.peeking ? 'end-peek' : c.mode === 'expanded' ? 'collapse' : 'hide'),
      },
    ],
  },
  { caps: '⌘W', desc: 'key.hide', binds: [{ key: 'w', meta: true, act: always('hide') }] },
  {
    caps: '1 2 3',
    desc: 'key.range',
    binds: [
      { key: '1', act: inPanel('range-session') },
      { key: '2', act: inPanel('range-today') },
      { key: '3', act: inPanel('range-week') },
    ],
  },
  { caps: '⌘E', desc: 'key.export', binds: [{ key: 'e', meta: true, act: always('export') }] },
  { caps: '⌘,', desc: 'key.settings', binds: [{ key: ',', meta: true, act: always('settings') }] },
  { caps: '⌘Q', desc: 'key.quit', binds: [{ key: 'q', meta: true, act: always('quit') }] },
  { caps: '?', desc: 'key.keys', binds: [{ key: '?', act: always('keys') }] },
]

const GLYPH: Record<string, string> = { ' ': 'space', ArrowDown: '↓', ArrowUp: '↑', Enter: '↵', Escape: 'esc' }

/** How a binding is printed in the keys list. */
export const capOf = (b: Binding): string => (b.meta ? '⌘' : '') + (GLYPH[b.key] ?? b.key.toUpperCase())

/** Rows for settings; the global peek key (handled by the OS, not keydown) comes last. */
export const keysList = (peekCaps: string): [string, string][] => [
  ...KEYMAP.map((r): [string, string] => [r.caps, t(r.desc)]),
  [peekCaps, t('key.peek')],
]

export function keyAction(e: KeyIn, ctx: KeyCtx): Action | null {
  if (e.ctrl || e.alt) return null
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key
  for (const row of KEYMAP) {
    for (const b of row.binds) {
      if (b.key !== key || !!b.meta !== e.meta) continue
      // Typing: only ⌘ shortcuts and esc. A focused button handles its own space/enter.
      if (e.target === 'field' && !b.meta && key !== 'Escape') return null
      if (e.target === 'button' && (key === ' ' || key === 'Enter')) return null
      return b.act(ctx)
    }
  }
  return null
}
