// Range tabs: `session · today · week`. Selected = bright + underlined; others dim.
import { h } from '../dom'
import type { RangeTab, TabId } from '../viewmodel'

export function createTabs(onPick: (id: TabId) => void) {
  const el = h('div', { class: 'tabs', attrs: { role: 'tablist', 'aria-label': 'range' } })
  const buttons = new Map<TabId, HTMLButtonElement>()
  for (const id of ['session', 'today', 'week'] as const) {
    if (buttons.size) el.append(h('span', { class: 'sep', attrs: { 'aria-hidden': 'true' } }, '·'))
    const b = h('button', { class: 'tab', attrs: { type: 'button', role: 'tab' } }, id)
    b.onclick = () => onPick(id)
    buttons.set(id, b)
    el.append(b)
  }

  function update(tabs: RangeTab[]) {
    for (const t of tabs) {
      const b = buttons.get(t.id)
      if (!b) continue
      b.textContent = t.text
      b.title = t.title
      b.classList.toggle('on', t.selected)
      b.setAttribute('aria-selected', String(t.selected))
      b.disabled = t.disabled
    }
  }

  return { el, update }
}
