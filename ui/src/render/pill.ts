// The pill: always visible (the top of the card). Window controls live here only: two stacked
// buttons on the right edge, each beside its own text line (no × — hide is esc / ⌘W / peek key).
//   ● bank-agent-lab              38:14  [■]
//     risk_engine.py · total 1:42:07     [▾]
// The whole text area toggles the panel on click; dragging it (≥ 4px) moves the window.
import { h } from '../dom'
import { gesture, IDLE, type Gesture } from '../gesture'
import { startWindowDrag } from '../platform'
import { expandButton, trackButton, type PillModel } from '../viewmodel'
import { setButton, tbtn } from './button'

export function createPill(on: { toggleExpand: () => void; track: () => void; allowAccessibility: () => void }) {
  const dot = h('span', { class: 'dot', attrs: { 'aria-hidden': 'true' } })
  const title = h('span', { class: 'pill-title' })
  const badge = h('span', { class: 'pill-badge' }) // dim "auto" before the time
  const timeValue = h('span')
  const time = h('span', { class: 'pill-time' }, badge, timeValue)
  // line 2: lead (detail / hint, ellipsizes first) · total (never cut)
  const lead = h('span', { class: 'pill-lead' })
  const nudge = h('button', { class: 'nudge', attrs: { type: 'button' }, title: 'no file names — macOS resets this after each rebuild' })
  nudge.onclick = on.allowAccessibility
  const sep = h('span', { class: 'pill-sep' }, '·')
  const totalLabel = h('span', { class: 'faint' })
  const totalValue = h('span')
  const total = h('span', { class: 'pill-total' }, totalLabel, ' ', totalValue)
  const sub = h('span', { class: 'pill-sub' }, lead, nudge, sep, total)
  const text = h('div', { class: 'pill-text' }, dot, title, time, h('span'), sub)
  const track = tbtn('', { cls: 'icon', onclick: on.track })
  const expand = tbtn('', { cls: 'icon', onclick: on.toggleExpand })
  const el = h('div', { class: 'pill' }, text, h('div', { class: 'pill-ctl' }, track, expand))

  // click vs drag — see gesture.ts
  let g: Gesture = IDLE
  const feed = (type: 'down' | 'move' | 'up' | 'cancel', e: PointerEvent) => {
    const r = gesture(g, { type, x: e.screenX, y: e.screenY })
    g = r.g
    if (r.action === 'drag') void startWindowDrag()
    if (r.action === 'click') on.toggleExpand()
  }
  text.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || (e.target as Element).closest('button')) return // the nudge is its own button
    text.setPointerCapture(e.pointerId)
    feed('down', e)
  })
  text.addEventListener('pointermove', (e) => feed('move', e))
  text.addEventListener('pointerup', (e) => feed('up', e))
  text.addEventListener('pointercancel', (e) => feed('cancel', e))

  function update(m: PillModel, tracking: boolean, expanded: boolean) {
    el.dataset.tone = m.tone
    dot.textContent = m.tone === 'idle' ? '○' : '●'
    dot.className = `dot tone-${m.tone}`
    title.textContent = m.title
    title.title = m.title
    timeValue.textContent = m.time ?? ''
    badge.textContent = m.badge ?? ''
    badge.hidden = !m.badge
    time.title = m.timeTitle ?? ''
    const isAx = m.lead?.kind === 'ax'
    lead.hidden = !m.lead || isAx
    nudge.hidden = !isAx
    if (m.lead) (isAx ? nudge : lead).textContent = m.lead.text
    lead.title = m.lead && !isAx ? m.lead.text : ''
    lead.className = `pill-lead${m.lead?.kind === 'hint' ? ' hint' : ''}`
    sep.hidden = !m.lead || !m.total
    total.hidden = !m.total
    totalLabel.textContent = m.total?.label ?? ''
    totalValue.textContent = m.total?.value ?? ''
    total.title = m.total?.title ?? ''
    const t = trackButton(tracking)
    setButton(track, t.text, t.title, t.kind)
    track.classList.add('icon')
    const x = expandButton(expanded)
    setButton(expand, x.text, x.title, 'neutral')
    expand.classList.add('icon')
    expand.setAttribute('aria-expanded', String(expanded))
    text.title = x.title
  }

  return { el, update }
}
