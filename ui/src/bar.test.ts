import { describe, expect, it } from 'vitest'
import { asciiBar, barFill, formatShare } from './bar'

describe('asciiBar', () => {
  it('fills proportionally with █ and pads with ░ to the width', () => {
    expect(asciiBar(0.5, 10)).toBe('█████░░░░░')
    expect(asciiBar(1, 4)).toBe('████')
    expect(asciiBar(0, 4)).toBe('░░░░')
  })
  it('always has exactly `width` characters', () => {
    for (const s of [0, 0.01, 0.333, 0.5, 0.999, 1]) expect([...asciiBar(s, 20)]).toHaveLength(20)
  })
  it('rounds to the nearest cell', () => {
    expect(barFill(0.38, 20)).toBe(8)
    expect(barFill(0.724, 20)).toBe(14)
  })
  it('shows at least one cell for any nonzero share', () => {
    expect(barFill(0.001, 20)).toBe(1)
  })
  it('clamps out-of-range and NaN shares', () => {
    expect(asciiBar(1.7, 4)).toBe('████')
    expect(asciiBar(-0.2, 4)).toBe('░░░░')
    expect(asciiBar(Number.NaN, 4)).toBe('░░░░')
  })
  it('returns empty string for non-positive width', () => {
    expect(asciiBar(0.5, 0)).toBe('')
  })
})

describe('formatShare (row % of in-use time)', () => {
  it('is a whole-number percent for the text fallback', () => {
    expect(formatShare(0.384)).toBe('38%')
    expect(formatShare(1)).toBe('100%')
  })
  it('never says 0% for a visible sliver', () => {
    expect(formatShare(0.001)).toBe('<1%')
    expect(formatShare(0)).toBe('0%')
  })
  it('rounds to whole percents (≤ 4 chars, fits the column)', () => {
    expect(formatShare(0.545)).toBe('55%')
    expect(formatShare(0.004)).toBe('<1%')
    expect(formatShare(0.005)).toBe('1%')
    expect(formatShare(0.999)).toBe('100%')
  })
  it('clamps and handles NaN', () => {
    expect(formatShare(1.4)).toBe('100%')
    expect(formatShare(Number.NaN)).toBe('0%')
  })
})
