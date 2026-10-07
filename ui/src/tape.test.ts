import { describe, expect, it } from 'vitest'
import { compactCount, TAPE_COMPACT, tapeCells, tapeClick, tapeGlyph } from './tape'
import { MIN, seg } from './testkit'

describe('tapeCells', () => {
  it('returns `count` empty cells when there are no segments', () => {
    const cells = tapeCells([], 5)
    expect(cells).toHaveLength(5)
    expect(cells.every((c) => c.tone === 'empty' && c.index === -1 && !c.live)).toBe(true)
  })

  it('maps a single focus segment to all-active cells', () => {
    const cells = tapeCells([seg({ start_ms: 0, end_ms: 10 * MIN })], 4)
    expect(cells.map((c) => c.tone)).toEqual(['active', 'active', 'active', 'active'])
    expect(cells.map((c) => c.index)).toEqual([0, 0, 0, 0])
  })

  it('spans first start to last end, proportionally', () => {
    const cells = tapeCells(
      [
        seg({ start_ms: 0, end_ms: 5 * MIN }),
        seg({ start_ms: 5 * MIN, end_ms: 7 * MIN, kind: 'away' }),
        seg({ start_ms: 7 * MIN, end_ms: 10 * MIN, kind: 'gap' }),
      ],
      10,
    )
    expect(cells.map((c) => c.tone).join(',')).toBe(
      'active,active,active,active,active,away,away,gap,gap,gap',
    )
  })

  it('gives each cell to the segment covering most of it', () => {
    // cell 0 = [0,10): seg A covers 0..4, seg B covers 4..10 → B wins
    const cells = tapeCells(
      [
        seg({ start_ms: 0, end_ms: 4, key: 'a' }),
        seg({ start_ms: 4, end_ms: 20, key: 'b', kind: 'glance' }),
      ],
      2,
    )
    expect(cells.map((c) => c.index)).toEqual([1, 1])
    expect(cells[0]?.tone).toBe('glance')
  })

  it('marks focus mostly spent passive as passive', () => {
    const cells = tapeCells([seg({ start_ms: 0, end_ms: 10, active_ms: 3, passive_ms: 7 })], 1)
    expect(cells[0]?.tone).toBe('passive')
  })

  it('carries the segment category', () => {
    const cells = tapeCells([seg({ start_ms: 0, end_ms: 10, category: 'ai' })], 1)
    expect(cells[0]?.category).toBe('ai')
  })

  it('honours an explicit window and leaves uncovered time empty', () => {
    const cells = tapeCells([seg({ start_ms: 0, end_ms: 10 })], 4, 0, 20)
    expect(cells.map((c) => c.tone)).toEqual(['active', 'active', 'empty', 'empty'])
  })
})

describe('live cell', () => {
  it('marks only the last cell live when asked', () => {
    const cells = tapeCells([seg({ start_ms: 0, end_ms: 10 })], 3, undefined, undefined, true)
    expect(cells.map((c) => c.live)).toEqual([false, false, true])
    expect(cells[2]?.tone).toBe('active') // keeps its tone for the tooltip/fallback
  })
  it('no live cell by default or without segments', () => {
    expect(tapeCells([seg({ start_ms: 0, end_ms: 10 })], 3).some((c) => c.live)).toBe(false)
    expect(tapeCells([], 3, undefined, undefined, true).some((c) => c.live)).toBe(false)
  })
})

describe('tapeGlyph', () => {
  it('has one glyph per tone', () => {
    expect(tapeGlyph('active')).toBe('▇')
    expect(tapeGlyph('passive')).toBe('▅')
    expect(tapeGlyph('glance')).toBe('▃')
    expect(tapeGlyph('away')).toBe('░')
    expect(tapeGlyph('gap')).toBe('·')
    expect(tapeGlyph('empty')).toBe(' ')
  })
})

describe('compact tape beside ▸ details (fits 340px in both languages)', () => {
  it('cells fill what the longest disclosure label leaves', () => {
    // row 314px; label: chars × 7.25px + 8px padding; 12px gap; each cell 6px + 1px gap
    expect(compactCount(314, 12)).toBe(Math.floor((314 - (12 * 7.25 + 8) - 12 + 1) / 7))
  })
  it('TAPE_COMPACT fits beside "▸ details" and "▸ ayrıntılar"', () => {
    for (const label of ['▸ Details', '▸ Ayrıntılar', '▾ ayrıntılar']) {
      const used = [...label].length * 7.25 + 8 + 12 + (TAPE_COMPACT * 7 - 1)
      expect(used, label).toBeLessThanOrEqual(314)
    }
    expect(TAPE_COMPACT).toBeGreaterThanOrEqual(25) // still a readable strip
  })
})

describe('tapeClick: a cell opens details and selects that segment', () => {
  it('opens details when collapsed and selects the segment', () => {
    expect(tapeClick({ detailsOpen: false, openSeg: null }, 4)).toEqual({ detailsOpen: true, openSeg: 4 })
  })
  it('already open: just selects (never toggles off)', () => {
    expect(tapeClick({ detailsOpen: true, openSeg: 4 }, 4)).toEqual({ detailsOpen: true, openSeg: 4 })
    expect(tapeClick({ detailsOpen: true, openSeg: 2 }, 7)).toEqual({ detailsOpen: true, openSeg: 7 })
  })
})
