// Builders for tests only — never imported by app code.
import type { SegmentDto, View } from './types'

export const MIN = 60_000

export function seg(p: Partial<SegmentDto> & Pick<SegmentDto, 'start_ms' | 'end_ms'>): SegmentDto {
  const kind = p.kind ?? 'focus'
  const dur = p.end_ms - p.start_ms
  const isActivity = kind === 'focus' || kind === 'glance'
  return {
    key: p.key ?? (kind === 'focus' || kind === 'glance' ? 'code:proj' : kind),
    label: p.label ?? (isActivity ? 'proj' : kind),
    category: p.category === undefined ? (isActivity ? 'code' : null) : p.category,
    kind,
    active_ms: p.active_ms ?? (isActivity ? dur : 0),
    passive_ms: p.passive_ms ?? 0,
    details: p.details ?? [],
    interruptions: p.interruptions ?? [],
    explain: p.explain ?? [],
    project: p.project ?? null,
    ...p,
  }
}

export function view(p: Partial<View> = {}): View {
  return {
    range: { kind: 'today' },
    total_ms: 0,
    active_ms: 0,
    passive_ms: 0,
    away_ms: 0,
    rows: [],
    segments: [],
    longest_focus_ms: 0,
    switches: 0,
    one_liner: '',
    categories: [],
    listening: [],
    not_shown: [],
    ...p,
  }
}
