// LED-meter cells drawn with CSS (fixed px, 1px gaps) so bars stay crisp at any DPR.
// The ASCII string stays the model: █ → lit cell, anything else → unlit.
import { h } from '../dom'

export function meter(ascii: string, label: string): HTMLElement {
  return h(
    'span',
    { class: 'meter', title: label, attrs: { role: 'img', 'aria-label': label } },
    ...[...ascii].map((c) => h('i', { class: c === '█' ? 'on' : 'off' })),
  )
}
