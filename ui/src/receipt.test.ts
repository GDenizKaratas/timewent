import { describe, expect, it } from 'vitest'
import { copyTarget, nextReceipt, resolveRange, type RangeChoice } from './receipt'
import type { SessionMeta, Status } from './types'

const st = (p: Partial<Status> = {}): Status => ({
  tracking: false, session_id: null, started_at_ms: null, elapsed_ms: 0, current: null,
  permissions: { accessibility: true }, auto: false, ...p,
})
const sessions: SessionMeta[] = [{ id: 9, started_at_ms: 2, ended_at_ms: 3 }, { id: 8, started_at_ms: 1, ended_at_ms: 2 }]

describe('receipt (§16.1: stop = receipt)', () => {
  const week: RangeChoice = { kind: 'week' }
  it('stop shows the receipt for the session that just ended, remembering the range choice', () => {
    expect(nextReceipt(null, { type: 'stopped', sessionId: 9, choice: week })).toEqual({ sessionId: 9, prevChoice: week })
  })
  it('clears on start, collapse (esc steps down through collapse) — and gives the old choice back', () => {
    const r = { sessionId: 9, prevChoice: week }
    for (const type of ['started', 'collapsed'] as const) expect(nextReceipt(r, { type })).toBeNull()
  })
  it('other events keep it', () => {
    const r = { sessionId: 9, prevChoice: null }
    expect(nextReceipt(r, { type: 'refreshed' })).toBe(r)
  })
  it('a second stop replaces it', () => {
    expect(nextReceipt({ sessionId: 8, prevChoice: null }, { type: 'stopped', sessionId: 9, choice: null })?.sessionId).toBe(9)
  })
})

describe('resolveRange (§16.4 default range)', () => {
  it('no explicit choice: session, or today when auto mode is on', () => {
    expect(resolveRange(null, st({ tracking: true, session_id: 9 }), sessions)).toEqual({ kind: 'session', id: 9 })
    expect(resolveRange(null, st({ tracking: true, session_id: 9, auto: true }), sessions)).toEqual({ kind: 'today' })
  })
  it('not tracking: the latest session (or today when there is none)', () => {
    expect(resolveRange(null, st(), sessions)).toEqual({ kind: 'session', id: 9 })
    expect(resolveRange(null, st(), [])).toEqual({ kind: 'today' })
  })
  it('the user\'s tab choice wins, also over auto mode', () => {
    expect(resolveRange('session', st({ tracking: true, session_id: 9, auto: true }), sessions)).toEqual({ kind: 'session', id: 9 })
    expect(resolveRange({ kind: 'week' }, st({ auto: true }), sessions)).toEqual({ kind: 'week' })
    expect(resolveRange({ kind: 'session', id: 8 }, st(), sessions)).toEqual({ kind: 'session', id: 8 })
  })
})

describe('copyTarget (copy = json of what is on screen)', () => {
  const effective = { kind: 'today' } as const
  it('normally: the current range', () => {
    expect(copyTarget({ past: null, receipt: null, effective })).toEqual({ kind: 'today' })
  })
  it('right after stop: the session that just ended', () => {
    expect(copyTarget({ past: null, receipt: { sessionId: 9, prevChoice: null }, effective })).toEqual({ kind: 'session', id: 9 })
  })
  it('viewing a past session from history: that session (wins over a stale receipt)', () => {
    expect(copyTarget({ past: { id: 7, label: 'x' }, receipt: { sessionId: 9, prevChoice: null }, effective })).toEqual({ kind: 'session', id: 7 })
  })
})
