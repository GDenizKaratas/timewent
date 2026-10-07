// The tape: a strip of LED cells, compact, in the same row as `▸ details` (PLAN "Tape beside
// details"). Each cell links to the segment it shows. Cell height mirrors ▇ active, ▅ passive, ▃ glance.
import { h } from '../dom'
import { clockRange, clockTime } from '../format'
import { t } from '../i18n'
import { tapeCells, tapeGlyph, TAPE_COMPACT, type Tone } from '../tape'
import type { SegmentDto } from '../types'

const ledCell = (tone: Tone) => h('i', { class: `led tone-${tone}` })

/** Fills the strip and its axis; returns the time span (`13:57–15:41`) for the row tooltip. */
export function renderTape(strip: HTMLElement, axis: HTMLElement, segments: readonly SegmentDto[], live: boolean): string {
  const cells = tapeCells(segments, TAPE_COMPACT, undefined, undefined, live)
  strip.setAttribute('role', 'img')
  strip.setAttribute('aria-label', `timeline ${cells.map((c) => tapeGlyph(c.tone)).join('')}`)
  strip.replaceChildren(
    ...cells.map((c) => {
      const s = segments[c.index]
      const cell = ledCell(c.tone)
      if (c.live) cell.classList.add('live')
      if (s && !s.key.startsWith('pass:')) {
        const label = s.kind === 'gap' ? t('not_tracked') : s.label
        cell.title = `${clockRange(s.start_ms, s.end_ms)}  ${label}${c.live ? ` · ${t('now')}` : ''}`
        cell.dataset.seg = String(c.index)
      }
      return cell
    }),
  )
  const first = segments[0]
  const last = segments[segments.length - 1]
  axis.replaceChildren(h('span', null, first ? clockTime(first.start_ms) : ''), h('span', null, last ? clockTime(last.end_ms) : ''))
  return first && last ? clockRange(first.start_ms, last.end_ms) : ''
}
