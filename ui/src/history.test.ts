import { afterEach, describe, expect, it } from 'vitest'
import { confirmLayout, historyRow, nextConfirm, nextPast, pastStrip, PAGE } from './history'
import { setLang } from './i18n'
import type { SessionOverview } from './types'

const at = (d: number, h: number, m: number) => new Date(2026, 9, d, h, m).getTime()
const closed: SessionOverview = { id: 8, started_at_ms: at(7, 0, 36), ended_at_ms: at(7, 0, 57), in_use_ms: 18 * 60_000, top_label: 'bank-agent-lab' }
const open: SessionOverview = { id: 9, started_at_ms: at(7, 9, 5), ended_at_ms: null, in_use_ms: 42 * 60_000, top_label: null }

describe('historyRow (§19)', () => {
  afterEach(() => setLang('en'))
  it('07.10  00:36–00:57  18m  bank-agent-lab, deletable', () => {
    expect(historyRow(closed)).toEqual({
      id: 8, date: '07.10', range: '00:36–00:57', inUse: '18m', label: 'bank-agent-lab', labelMissing: false, open: false, deletable: true,
    })
  })
  it('the open session: ● now, never deletable; no top row yet → a dim dash', () => {
    expect(historyRow(open)).toMatchObject({ range: '09:05–', label: '—', labelMissing: true, open: true, deletable: false })
    expect(historyRow(closed).labelMissing).toBe(false)
  })
  it('compact time follows the language', () => {
    setLang('tr')
    expect(historyRow(closed).inUse).toBe('18dk')
  })
  it('pages of 20', () => {
    expect(PAGE).toBe(20)
  })
})

describe('delete confirm (inline, one at a time)', () => {
  it('ask → confirm shows for that row; cancel or done clears', () => {
    const a = nextConfirm(null, { type: 'ask', id: 8 })
    expect(a).toBe(8)
    expect(nextConfirm(a, { type: 'cancel' })).toBeNull()
    expect(nextConfirm(a, { type: 'done' })).toBeNull()
  })
  it('asking on another row moves the confirm there', () => {
    expect(nextConfirm(8, { type: 'ask', id: 7 })).toBe(7)
  })
})

describe('confirm keeps the row identifiable (§19 fix)', () => {
  it('the row stays, highlighted; the confirm sits under it', () => {
    expect(confirmLayout(8, 8)).toEqual({ highlight: true, confirmBelow: true })
    expect(confirmLayout(7, 8)).toEqual({ highlight: false, confirmBelow: false })
    expect(confirmLayout(7, null)).toEqual({ highlight: false, confirmBelow: false })
  })
})

describe('past-session context strip (§19)', () => {
  afterEach(() => setLang('en'))
  it('opening a history row shows that session with a strip; back returns to history', () => {
    const p = nextPast(null, { type: 'open', session: closed })
    expect(p).toEqual({ id: 8, label: '07.10 · 00:36–00:57' })
    expect(nextPast(p, { type: 'back' })).toBeNull()
  })
  it('a range tab pick or collapse leaves the past session too', () => {
    const p = nextPast(null, { type: 'open', session: closed })
    expect(nextPast(p, { type: 'pick-range' })).toBeNull()
    expect(nextPast(p, { type: 'collapse' })).toBeNull()
  })
  it('strip model: label + back button text', () => {
    expect(pastStrip({ id: 8, label: '07.10 · 00:36–00:57' })).toEqual({ label: '07.10 · 00:36–00:57', back: '← History' })
    setLang('tr')
    expect(pastStrip({ id: 8, label: 'x' }).back).toBe('← Geçmiş')
  })
})
