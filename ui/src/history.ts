// §19 history tab + the past-session context strip. Pure.
import { clockTime, formatCompact } from './format'
import { t } from './i18n'
import type { SessionOverview } from './types'

export const PAGE = 20

const pad2 = (n: number) => String(n).padStart(2, '0')
const dayMonth = (ms: number) => {
  const d = new Date(ms)
  return `${pad2(d.getDate())}.${pad2(d.getMonth() + 1)}`
}

export type HistoryRow = {
  id: number
  date: string // 07.10
  range: string // 00:36–00:57 (open: 09:05–)
  inUse: string // compact
  label: string // top row, '—' when none yet
  labelMissing: boolean // render the dash dim
  open: boolean // ● now
  deletable: boolean // never the open session
}

export function historyRow(o: SessionOverview): HistoryRow {
  const open = o.ended_at_ms === null
  return {
    id: o.id,
    date: dayMonth(o.started_at_ms),
    range: `${clockTime(o.started_at_ms)}–${open || o.ended_at_ms === null ? '' : clockTime(o.ended_at_ms)}`,
    inUse: formatCompact(o.in_use_ms),
    label: o.top_label ?? '—',
    labelMissing: o.top_label === null,
    open,
    deletable: !open,
  }
}

/** Inline delete confirm: the id being confirmed, or null. One at a time. */
export type ConfirmEvent = { type: 'ask'; id: number } | { type: 'cancel' } | { type: 'done' }
export function nextConfirm(_c: number | null, e: ConfirmEvent): number | null {
  return e.type === 'ask' ? e.id : null
}

/** Viewing a past session from history: the panel shows it with a context strip. */
export type Past = { id: number; label: string } | null
export type PastEvent =
  | { type: 'open'; session: SessionOverview }
  | { type: 'back' } // [← history] or esc → back to the history tab
  | { type: 'pick-range' } // a range tab wins
  | { type: 'collapse' }

export function nextPast(_p: Past, e: PastEvent): Past {
  if (e.type !== 'open') return null
  const r = historyRow(e.session)
  return { id: e.session.id, label: `${r.date} · ${r.range}` }
}

export const pastStrip = (p: NonNullable<Past>) => ({ label: p.label, back: t('back_history') })

/** The row being confirmed stays visible (highlighted) with the confirm directly under it. */
export const confirmLayout = (rowId: number, confirming: number | null) => {
  const on = confirming === rowId
  return { highlight: on, confirmBelow: on }
}
