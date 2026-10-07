// "where your time went": top rows with LED meters, then `show all (+n)` when there are more.
import { h } from '../dom'
import { t } from '../i18n'
import { capDetails, moreToggle, type WhereSummary } from '../viewmodel'
import { tbtn } from './button'
import { meter } from './meter'

export function renderWhere(
  el: HTMLElement,
  s: WhereSummary,
  open: ReadonlySet<string>,
  on: { toggleAll: () => void; toggleDetails: (key: string) => void },
  showingAll: boolean,
  allDetails: ReadonlySet<string>, // rows showing every detail (past the cap of 5)
) {
  if (s.rows.length === 0) {
    el.replaceChildren(h('div', { class: 'empty' }, t('nothing_tracked')))
    return
  }
  const rows = s.rows.flatMap((r): (Node | string)[] => {
    const expandable = r.expandable
    const isOpen = expandable && open.has(r.key)
    const line = h(
      'div',
      {
        class: `row${r.live ? ' live' : ''}${expandable ? ' click' : ''}${isOpen ? ' open' : ''}`,
        attrs: expandable ? { 'data-key': r.key } : {},
        title: expandable ? t('row_title', { label: r.label }) : r.label,
      },
      h('span', { class: 'caret' }, expandable ? (isOpen ? '▾' : '▸') : ''),
      h('span', { class: 'label' }, r.label),
      meter(r.bar, t('share_title', { p: r.pct })),
      h('span', { class: 'pct' }, r.pct),
      h('span', { class: 'time' }, r.time),
    )
    if (!isOpen) return [line]
    // project / activity: what it was made of first, then the files / pages (5, then +n more)
    const cap = capDetails(r.details, allDetails.has(r.key))
    return [
      line,
      r.breakdown ? h('div', { class: 'breakdown' }, r.breakdown) : '',
      ...cap.shown.map((d) =>
        h('div', { class: 'row sub' }, h('span', { class: 'label', title: d.detail }, d.detail), h('span', { class: 'time' }, d.time)),
      ),
      cap.toggle
        ? h(
            'div',
            { class: 'sub-more' },
            tbtn(cap.toggle, {
              kind: 'quiet',
              cls: 'text',
              onclick: (e) => {
                e.stopPropagation() // not a click on the row
                on.toggleDetails(r.key)
              },
            }),
          )
        : '',
    ]
  })
  const extra: (Node | string)[] = []
  const toggle = moreToggle(s, showingAll)
  if (toggle) extra.push(tbtn(toggle, { kind: 'quiet', cls: 'text', onclick: on.toggleAll }))
  el.replaceChildren(h('div', { class: 'rows' }, ...rows), extra.length ? h('div', { class: 'more' }, ...extra) : '')
}
