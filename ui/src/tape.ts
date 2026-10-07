// Tape strip model: squeeze chronological segments into a fixed number of character cells.
import { DICTS } from './i18n'
import type { Category, SegmentDto } from './types'

export type Tone = 'active' | 'passive' | 'glance' | 'away' | 'gap' | 'empty'
export type TapeCell = { tone: Tone; category: Category | null; index: number; live: boolean }

export function segmentTone(s: SegmentDto): Exclude<Tone, 'empty'> {
  switch (s.kind) {
    case 'focus':
      return s.passive_ms > s.active_ms ? 'passive' : 'active'
    case 'glance':
      return 'glance'
    case 'away':
      return 'away'
    case 'gap':
      return 'gap'
  }
}

/**
 * Each cell covers an equal slice of [from, to) and shows the segment overlapping it most
 * (earliest wins ties). Lossy by design: a 9s glance in a 3h day may not get a cell.
 */
export function tapeCells(
  segments: readonly SegmentDto[],
  count: number,
  from?: number,
  to?: number,
  live = false, // tracking now: the last cell is "now" (green)
): TapeCell[] {
  const empty = (): TapeCell => ({ tone: 'empty', category: null, index: -1, live: false })
  const first = segments[0]
  const last = segments[segments.length - 1]
  if (!first || !last || count <= 0) return Array.from({ length: Math.max(0, count) }, empty)

  const t0 = from ?? first.start_ms
  const t1 = to ?? last.end_ms
  const step = (t1 - t0) / count
  const cells: TapeCell[] = []
  let j = 0 // segments are chronological, so scan forward only
  for (let c = 0; c < count; c++) {
    const a = t0 + c * step
    const b = a + step
    while (j < segments.length && (segments[j]?.end_ms ?? 0) <= a) j++
    let best = -1
    let bestOverlap = 0
    for (let k = j; k < segments.length; k++) {
      const s = segments[k]
      if (!s || s.start_ms >= b) break
      const overlap = Math.min(b, s.end_ms) - Math.max(a, s.start_ms)
      if (overlap > bestOverlap) {
        best = k
        bestOverlap = overlap
      }
    }
    const s = segments[best]
    cells.push(s ? { tone: segmentTone(s), category: s.category, index: best, live: false } : empty())
  }
  const lastCell = cells[cells.length - 1]
  if (live && lastCell) lastCell.live = true
  return cells
}

const GLYPHS: Record<Tone, string> = {
  active: '▇',
  passive: '▅',
  glance: '▃',
  away: '░',
  gap: '·',
  empty: ' ',
}

export const tapeGlyph = (tone: Tone): string => GLYPHS[tone]

// ── compact tape beside `▸ details` (PLAN "Tape beside details") ───────────────
const CH_PX = 7.25 // 12px mono advance
const LABEL_PAD_PX = 8 // disclosure button padding
const ROW_GAP_PX = 12

/** Cells (6px + 1px gap) that fit in `rowPx` next to a disclosure label of `labelChars`. */
export const compactCount = (rowPx: number, labelChars: number): number =>
  Math.floor((rowPx - (labelChars * CH_PX + LABEL_PAD_PX) - ROW_GAP_PX + 1) / 7)

/** One fixed count for every language: sized for the longest `▸ <details>` label. */
export const TAPE_COMPACT = compactCount(314, Math.max(...Object.values(DICTS).map((d) => [...`▸ ${d.details}`].length)))

/** A cell click opens details (if closed) and selects that segment in recent activity. */
export const tapeClick = (_s: { detailsOpen: boolean; openSeg: number | null }, seg: number) => ({
  detailsOpen: true,
  openSeg: seg,
})
