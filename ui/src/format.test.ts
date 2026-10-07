import { afterEach, describe, expect, it } from 'vitest'
import { setLang } from './i18n'
import { clockRange, clockTime, formatClockShort, formatCompact, formatDuration, formatLong } from './format'

const at = (h: number, m: number, s = 0) => new Date(2026, 9, 6, h, m, s).getTime()

describe('formatDuration', () => {
  it('shows m:ss under an hour', () => {
    expect(formatDuration(0)).toBe('0:00')
    expect(formatDuration(9_000)).toBe('0:09')
    expect(formatDuration((38 * 60 + 14) * 1000)).toBe('38:14')
  })
  it('shows h:mm:ss from an hour on', () => {
    expect(formatDuration((3600 + 24 * 60 + 33) * 1000)).toBe('1:24:33')
    expect(formatDuration(10 * 3600 * 1000)).toBe('10:00:00')
  })
  it('floors partial seconds and clamps negatives', () => {
    expect(formatDuration(1_999)).toBe('0:01')
    expect(formatDuration(-5_000)).toBe('0:00')
    expect(formatDuration(Number.NaN)).toBe('0:00')
  })
})

describe('formatCompact', () => {
  it('uses seconds under a minute', () => {
    expect(formatCompact(0)).toBe('0s')
    expect(formatCompact(9_400)).toBe('9s')
    expect(formatCompact(59_999)).toBe('59s')
  })
  it('uses whole minutes under an hour', () => {
    expect(formatCompact(60_000)).toBe('1m')
    expect(formatCompact(16 * 60_000 + 40_000)).toBe('16m')
  })
  it('uses h + padded minutes from an hour on', () => {
    expect(formatCompact(3600_000 + 4 * 60_000)).toBe('1h04m')
    expect(formatCompact(12 * 3600_000 + 30 * 60_000)).toBe('12h30m')
  })
})

describe('formatCompact in turkish (sa / dk / sn)', () => {
  afterEach(() => setLang('en'))
  it('uses turkish units; clock style stays the same', () => {
    setLang('tr')
    expect(formatCompact(42_000)).toBe('42sn')
    expect(formatCompact(13 * 60_000)).toBe('13dk')
    expect(formatCompact(3600_000 + 4 * 60_000)).toBe('1sa04dk')
    expect(formatDuration((3600 + 38 * 60 + 27) * 1000)).toBe('1:38:27')
  })
})

describe('formatClockShort (h:mm, receipts)', () => {
  it('hours and minutes only', () => {
    expect(formatClockShort((74 * 60 + 59) * 1000)).toBe('1:14')
    expect(formatClockShort(42 * 60_000)).toBe('0:42')
    expect(formatClockShort(0)).toBe('0:00')
  })
})

describe('formatLong (exact, like core explain lines)', () => {
  afterEach(() => setLang('en'))
  it('every non-zero unit: 1m35s / 1h02m03s; turkish spaced: 1dk 35sn', () => {
    expect(formatLong(95_000)).toBe('1m35s')
    expect(formatLong(3_723_000)).toBe('1h2m3s')
    expect(formatLong(12_000)).toBe('12s')
    setLang('tr')
    expect(formatLong(95_000)).toBe('1dk 35sn')
    expect(formatLong(3_723_000)).toBe('1sa 2dk 3sn')
  })
})

describe('clock', () => {
  it('formats local 24h time', () => {
    expect(clockTime(at(9, 5))).toBe('09:05')
    expect(clockTime(at(14, 2, 59))).toBe('14:02')
  })
  it('formats a local range with an en dash', () => {
    expect(clockRange(at(14, 2), at(14, 18))).toBe('14:02–14:18')
  })
})
