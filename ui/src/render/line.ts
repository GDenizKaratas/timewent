// §20 summary lines: one line, always. label dim · name bright (the only part that shrinks,
// with an ellipsis) · value bright (nowrap, compact). Parts are joined by a dim " · ".
import { h } from '../dom'
import type { LinePart } from '../viewmodel'

export function renderLine(el: HTMLElement, parts: LinePart[]) {
  el.classList.add('line')
  el.replaceChildren(
    ...parts.flatMap((p, i) => [
      i ? h('span', { class: 'line-sep' }, '·') : '',
      h(
        'span',
        { class: `line-part${p.name ? ' shrink' : ''}` },
        p.label ? h('span', { class: 'line-label' }, p.label) : '',
        p.name ? h('span', { class: 'line-name', title: p.name }, p.name) : '',
        p.value ? h('span', { class: 'line-value' }, p.value) : '',
      ),
    ]),
  )
}
