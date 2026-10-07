// "recent activity": latest segment first; click one to read why it is what it is.
import { fill, h } from '../dom'
import { t } from '../i18n'
import type { ActivityItem } from '../viewmodel'

const MAX_ITEMS = 80 // a full day can have hundreds; older ones are a scroll away in export

export function renderActivity(el: HTMLElement, items: ActivityItem[], openIndex: number | null) {
  if (items.length === 0) {
    el.replaceChildren(h('div', { class: 'empty' }, t('no_segments')))
    return
  }
  const shown = items.slice(0, MAX_ITEMS)
  fill(
    el,
    ...shown.flatMap((it) => {
      const isOpen = it.index === openIndex
      const line = h(
        'div',
        { class: `act click k-${it.tone}${isOpen ? ' open' : ''}`, attrs: { 'data-seg': String(it.index) } },
        h('span', { class: 'caret' }, isOpen ? '▾' : '▸'),
        h('span', { class: 'what', title: it.what }, it.what),
        h('span', { class: 'when' }, it.when),
        h('span', { class: 'dur' }, it.howLong),
      )
      if (!isOpen) return [line]
      const lines = it.lines.length > 0 ? it.lines : [t('nothing_to_explain')]
      return [
        line,
        h(
          'div',
          { class: 'explain' },
          ...lines.map((l) => h('div', null, l)),
          ...it.interruptions.map((d) => kv(`↯ ${d.detail}`, d.time)),
          ...it.details.map((d) => kv(d.detail, d.time)),
        ),
      ]
    }),
    items.length > MAX_ITEMS ? h('div', { class: 'empty' }, t('older', { n: items.length - MAX_ITEMS })) : null,
  )
}

const kv = (k: string, v: string) => h('div', { class: 'detail' }, h('span', null, k), h('span', null, v))
