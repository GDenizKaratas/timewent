// Pure View/Status → display models. No DOM here.
import { asciiBar, formatShare } from './bar'
import { lang, t } from './i18n'
import { clockRange, clockTime, formatClockShort, formatCompact, formatDuration, formatLong } from './format'
import { segmentTone, type Tone } from './tape'
import type { Category, Layout, Presence, Range, Row, RowKind, SessionMeta, Status, View } from './types'

export type DetailLine = { detail: string; time: string }

export type WhereRow = {
  key: string
  label: string
  category: Category
  kind: RowKind
  time: string
  bar: string // ASCII model (█/░), rendered as LED cells
  pct: string // share of in-use time (same denominator as the header), shown dim next to the meter
  live: boolean // the row you're on right now → green meter
  breakdown: string | null // project: "code 38m · ai 12m"; activity: "VS Code 38m · iTerm2 10m"
  details: DetailLine[]
  expandable: boolean
}

const CATEGORY_KEY = {
  code: 'cat_code', ai: 'cat_ai', docs: 'cat_docs', web: 'cat_web', media: 'cat_media',
  social: 'cat_social', comms: 'cat_comms', design: 'cat_design', notes: 'cat_notes', app: 'cat_app',
} as const

/** Project breakdowns are categories (translated); activity breakdowns are member names (data). */
function breakdownLine(r: Row): string | null {
  if (r.kind === 'context' || r.breakdown.length === 0) return null
  return r.breakdown
    .map((b) => {
      const cat = r.kind === 'project' ? CATEGORY_KEY[b.key as Category] : undefined
      return `${cat ? t(cat) : b.label} ${formatCompact(b.ms)}`
    })
    .join(' · ')
}

/** Passthrough contexts (`pass:*`) are never "time spent": never a row. */
export const isPassthrough = (key: string): boolean => key.startsWith('pass:')

export type WhereSummary = { rows: WhereRow[]; more: number }

/** Default view: top `limit` rows (or all), the rest counted. Away lives in the how block. */
export function whereSummary(
  view: View,
  opts: { limit: number; showAll: boolean; liveKey: string | null },
  barWidth: number,
): WhereSummary {
  const all = view.rows.filter((r) => !isPassthrough(r.key)).map((r): WhereRow => {
    const breakdown = breakdownLine(r)
    return {
      key: r.key,
      label: r.label,
      category: r.category,
      kind: r.kind,
      time: formatDuration(r.ms),
      bar: asciiBar(r.share, barWidth),
      pct: formatShare(r.share),
      live: r.key === opts.liveKey,
      breakdown,
      details: r.details.map((d) => ({ detail: d.detail, time: formatCompact(d.ms) })), // dim → compact
      expandable: breakdown !== null || r.details.length > 0,
    }
  })
  const rows = opts.showAll ? all : all.slice(0, opts.limit)
  return {
    rows,
    more: all.length - rows.length,
  }
}

export type HowLine = {
  kind: 'away' | 'listen'
  key: string // dim key column (~7ch)
  value: string // normal weight, one line; ellipsizes first
  source?: string // ♪ lines: "· YouTube", never cut
  time?: string // ♪ lines: right-aligned compact time
  title?: string // tooltip with the full meaning
}

/** Under `▸ details` (DESIGN §11.2): away · ♪ (≤ 2). Lines with nothing to say are omitted. Kinds of time
 *  live in the in-use tooltip; focus is in the summary sentence. */
export function howLines(v: View): HowLine[] {
  const out: HowLine[] = []
  if (v.away_ms > 0) out.push({ kind: 'away', key: t('how_away'), value: formatCompact(v.away_ms) })
  for (const l of v.listening.slice(0, 2)) {
    const full = l.title ? `${l.title} · ${l.label}` : l.label
    out.push({
      kind: 'listen',
      key: '♪',
      value: l.title ?? l.label,
      ...(l.title ? { source: l.label } : {}),
      time: formatCompact(l.ms),
      title: `${full} — ${lcFirst(t('listening_title'))}`,
    })
  }
  return out
}

/** General tab hint for auto start, from Prefs.auto_split_after_s. */
export const autoHint = (splitAfterS: number): string => t('auto_hint', { d: formatCompact(splitAfterS * 1000) })

/** The row to light green: the current context, only if this range contains the running session. */
export function liveKey(st: Status | null, range: Range): string | null {
  const c = st?.current
  if (!st?.tracking || !c || c.presence === 'away' || isPassthrough(c.key)) return null
  if (range.kind === 'session' && range.id !== st.session_id) return null
  return c.key
}

/** One recent-activity row, in reading order: what → when → how long. */
export type ActivityItem = {
  what: string // label / top detail (+ " · glance"); away/gap say so themselves
  when: string // local HH:MM–HH:MM
  howLong: string // compact: 9s, 16m, 1h04m
  index: number // position in view.segments (chronological)
  tone: Exclude<Tone, 'empty'>
  lines: string[] // explain, as `# …` comments
  details: DetailLine[]
  interruptions: DetailLine[]
}

export function recentActivity(view: View): ActivityItem[] {
  const items: ActivityItem[] = view.segments.map((s, index) => {
    const top = s.details[0]?.detail
    // gaps: sleep, app closed, between sessions — say so honestly; away arrives labelled "away"
    const what = s.kind === 'gap' ? t('not_tracked') : top ? `${s.label} / ${top}` : s.label
    return {
      what: s.kind === 'glance' ? `${what} · ${t('glance')}` : what,
      when: clockRange(s.start_ms, s.end_ms),
      howLong: formatCompact(s.end_ms - s.start_ms),
      index,
      tone: segmentTone(s),
      lines: s.explain.map((l) => `# ${l}`),
      details: s.details.map((d) => ({ detail: d.detail, time: formatCompact(d.ms) })),
      interruptions: s.interruptions.map((i) => ({ detail: i.label, time: formatCompact(i.ms) })),
    }
  })
  // pass:* (timewent itself, etc.) is never shown; with count_self on it arrives as app:timewent
  return items.filter((it) => !isPassthrough(view.segments[it.index]?.key ?? '')).reverse()
}

export type PillLead = { text: string; kind: 'detail' | 'hint' | 'ax' } // ax = clickable nudge
export type PillModel = {
  tone: 'idle' | Presence
  title: string // line 1: what you're on now (never the brand while tracking)
  time: string | null // line 1, right
  timeTitle: string | null // says what that time is
  badge: string | null // dim "auto" right before the time (DESIGN §11.7)
  lead: PillLead | null // line 2, left, ellipsizes first; never repeats line 1
  total: { label: string; value: string; title: string } | null // line 2, right: never cut
}

const norm = (s: string) => s.trim().toLowerCase()
/** A dictionary label used mid-sentence: first letter lowered (tr-aware). */
const lcFirst = (s: string) => s.charAt(0).toLocaleLowerCase(lang()) + s.slice(1)

/** A detail worth a second line: present, and not just the label again. */
function distinctDetail(label: string, detail: string | null): string | null {
  if (!detail || !detail.trim()) return null
  return norm(detail).includes(norm(label)) ? null : detail.trim()
}

// fits beside the total at 340px; the pill's tooltip carries the why
const BRAND = 'timewent'

const axNudge = (): PillLead => ({ text: t('allow_ax'), kind: 'ax' })

export function pillModel(
  st: Status,
  nowMs: number,
  extra: { ended?: { in_use_ms: number } | null; todayInUseMs?: number | null } = {},
): PillModel {
  if (!st.tracking) {
    const hint = (text: string): PillLead => ({ text, kind: 'hint' })
    // DESIGN §11.1: not tracking → the brand (a proper noun, never translated) on line 1
    const today = extra.todayInUseMs ?? 0
    const line2 = extra.ended
      ? `${t('ended_in_use')} ${formatDuration(extra.ended.in_use_ms)}`
      : today > 0
        ? t('today_in_use', { t: formatDuration(today) })
        : t('start_hint')
    return { tone: 'idle', title: BRAND, time: null, timeTitle: null, badge: null, lead: hint(line2), total: null }
  }
  const total = { label: t('total'), value: formatDuration(st.elapsed_ms), title: t('total_title') }
  const ax = st.permissions.accessibility ? null : axNudge()
  const badge = st.auto ? t('auto_badge') : null
  const c = st.current
  if (!c) return { tone: 'active', title: '…', time: null, timeTitle: null, badge, lead: ax, total }
  if (c.presence === 'away') {
    return {
      tone: 'away',
      title: c.since_ms === null ? t('away') : t('away_since', { t: clockTime(c.since_ms) }),
      time: c.since_ms === null ? null : formatDuration(nowMs - c.since_ms),
      timeTitle: c.since_ms === null ? null : t('away_for'),
      badge,
      lead: ax,
      total,
    }
  }
  // DESIGN §11.1: a project or activity names the line; the app/site moves to line 2
  const title = c.project ?? c.label
  const second = (c.project ? distinctDetail(title, c.label) : null) ?? distinctDetail(title, c.detail)
  return {
    tone: c.presence,
    title,
    time: formatDuration(c.context_ms),
    timeTitle: t('on_context', { x: title }),
    badge,
    lead: ax ?? (second ? { text: second, kind: 'detail' } : null),
    total,
  }
}

/** Tracking and the range contains the running session → the tape's last cell is "now". */
export function rangeIsLive(st: Status | null, range: Range): boolean {
  if (!st?.tracking) return false
  return range.kind !== 'session' || range.id === st.session_id
}

export type TrackButton = { text: string; title: string; kind: 'primary' | 'neutral' }

export const trackButton = (tracking: boolean): TrackButton =>
  tracking
    ? { text: '■', title: t('stop_title'), kind: 'neutral' }
    : { text: '▶', title: t('start_title'), kind: 'primary' }

export const toLayout = (mode: 'collapsed' | 'expanded'): Layout => (mode === 'collapsed' ? 'pill' : 'expanded')
export const fromLayout = (layout: Layout): 'collapsed' | 'expanded' => (layout === 'pill' ? 'collapsed' : 'expanded')

export const expandButton = (expanded: boolean): { text: string; title: string } =>
  expanded ? { text: '▴', title: t('collapse_title') } : { text: '▾', title: t('expand_title') }

/** One part of a summary line (DESIGN §11.2): `label` dim; `name` bright and the only part that shrinks
 *  (ellipsis); `value` bright, nowrap, compact. Parts are joined by a dim ` · `. */
export type LinePart = { label?: string; name?: string; value?: string }

/** DESIGN §11.2: the one line under the rows — `Most: <top row> · longest focus 21m`. */
export function summarySentence(v: View): { parts: LinePart[]; title: string } | null {
  const parts: LinePart[] = []
  const top = v.rows.find((r) => !isPassthrough(r.key))
  if (top) parts.push({ label: t('most_label'), name: top.label })
  if (v.longest_focus_ms > 0) parts.push({ label: t('longest_focus'), value: formatCompact(v.longest_focus_ms) })
  return parts.length ? { parts, title: t('switches', { n: v.switches }) } : null
}

/** DESIGN §11.4 receipt line 2: top row · away. In-use is in the heading and focus in the how block,
 *  right below — so the receipt stays exactly two lines at 340px. */
export function receiptParts(v: View): LinePart[] {
  const parts: LinePart[] = []
  const top = v.rows.find((r) => !isPassthrough(r.key))
  if (top) parts.push({ name: top.label, value: formatClockShort(top.ms) })
  if (v.away_ms > 0) parts.push({ label: t('away'), value: formatCompact(v.away_ms) }) // mid-sentence: lowercase
  return parts
}

const DETAILS_CAP = 5

/** DESIGN §11.2: an open row shows 5 details, then `+n more` (inline toggle) — same rule as the rows. */
export function capDetails(d: DetailLine[], showAll: boolean): { shown: DetailLine[]; toggle: string | null } {
  if (d.length <= DETAILS_CAP) return { shown: d, toggle: null }
  return showAll ? { shown: d, toggle: t('show_less') } : { shown: d.slice(0, DETAILS_CAP), toggle: t('more_n', { n: d.length - DETAILS_CAP }) }
}

/** Inline rows toggle: `show all (+n)` / `show less`, or nothing when everything fits. */
export function moreToggle(s: WhereSummary, showingAll: boolean): string | null {
  if (s.more > 0) return t('show_all', { n: s.more })
  return showingAll && s.rows.length > 5 ? t('show_less') : null
}

/** File-name friendly range name (export). */
export const rangeKey = (r: Range): string => (r.kind === 'session' ? `session-${r.id}` : r.kind)

export type TabId = 'session' | 'today' | 'week'
export type RangeTab = { id: TabId; text: string; title: string; selected: boolean; disabled: boolean }

const TAB_KEYS = {
  session: ['tab_session', 'tab_session_title'],
  today: ['tab_today', 'tab_today_title'],
  week: ['tab_week', 'tab_week_title'],
} as const

/** Footer tabs. `session` = the latest session; disabled until one exists. */
export function rangeTabs(effective: Range, hasSession: boolean, noneSelected = false): RangeTab[] {
  return (['session', 'today', 'week'] as const).map((id) => ({
    id,
    text: t(TAB_KEYS[id][0]),
    title: t(TAB_KEYS[id][1]),
    selected: !noneSelected && effective.kind === id, // DESIGN §11.5: a past session selects no tab
    disabled: id === 'session' && !hasSession,
  }))
}

/** The range a tab selects; `null` = follow the latest session. */
export const tabRange = (id: TabId): Range | null => (id === 'session' ? null : { kind: id })

/** "In use" = active + passive (reading). Away and gaps never count. */
export const inUseMs = (v: View): number => v.active_ms + v.passive_ms

export function inUseTitle(v: View): string {
  const a = formatDuration(v.active_ms)
  const r = formatDuration(v.passive_ms)
  const head = v.away_ms > 0 ? t('in_use_title_away', { a, r, w: formatDuration(v.away_ms) }) : t('in_use_title', { a, r })
  // every kind of time with its share (DESIGN §8.3) — on its own line, the panel doesn't show them
  const kinds = v.categories.map((c) => `${t(CATEGORY_KEY[c.category])} ${formatShare(c.share)}`).join(' · ')
  // DESIGN §8.5: in use, but credited to no row — say so, or the visible shares look short
  const hidden = v.not_shown.map((n) => `${n.label} ${formatLong(n.ms)}`).join(' · ')
  return [head, kinds, hidden ? t('not_shown', { x: hidden }) : ''].filter(Boolean).join('\n')
}

/** Never started, or nothing in range while idle → show the `$ see where your time went` prompt. */
export const isFreshState = (st: Status | null, v: View): boolean =>
  !st?.tracking && v.segments.length === 0

const JUST_ENDED_MS = 15 * 60_000

/** Right after stop, while looking at that session → "session ended — here's where your time went". */
export function justEnded(st: Status | null, sessions: readonly SessionMeta[], range: Range, nowMs: number): boolean {
  if (st?.tracking || range.kind !== 'session') return false
  const s = sessions.find((x) => x.id === range.id)
  return !!s && s.ended_at_ms !== null && nowMs - s.ended_at_ms < JUST_ENDED_MS
}
