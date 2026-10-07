// DESIGN §11.2 "how" block: a key/value list — dim key column (~7ch), normal-weight value, one line each.
//   away    13:00
//   focus   longest 21m · 4 switches
//   kind    code 63% · ai 20% · web 10%
//   ♪       lofi beats to code to · YouTube   33m
import { h } from '../dom'
import type { HowLine } from '../viewmodel'

export function renderHow(el: HTMLElement, lines: HowLine[]) {
  el.hidden = lines.length === 0
  el.replaceChildren(
    ...lines.map((l) =>
      h(
        'div',
        { class: `kv kv-${l.kind}`, ...(l.title ? { title: l.title } : {}) },
        h('span', { class: 'kv-key' }, l.key),
        h(
          'span',
          { class: 'kv-value' },
          h('span', { class: 'kv-main' }, l.value), // ellipsizes first
          l.source ? h('span', { class: 'kv-sep' }, ' · ') : '',
          l.source ? h('span', { class: 'kv-src' }, l.source) : '',
        ),
        h('span', { class: 'kv-time' }, l.time ?? ''),
      ),
    ),
  )
}
