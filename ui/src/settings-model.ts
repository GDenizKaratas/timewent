// Threshold editing (§19): human labels with units in words; the raw config key only in the
// tooltip. Three knobs visible, the rest under `▸ advanced`.
import { lang, t, type Key } from './i18n'
import type { Config, Prefs } from './types'

type ConfigField =
  | 'poll_interval_ms' | 'passive_after_s' | 'away_after_s' | 'gap_after_s' | 'transient_max_s' | 'glance_max_s'
  | 'support_window_s'
export type ThresholdField = ConfigField | 'auto_split_after_s' // the last one is a pref

type Spec = { field: ThresholdField; label: Key; unit: Key; title: Key; advanced: boolean }

export const THRESHOLDS: readonly Spec[] = [
  { field: 'passive_after_s', label: 'lb_passive', unit: 'u_after_s', title: 'tt_passive', advanced: false },
  { field: 'away_after_s', label: 'lb_away', unit: 'u_after_s', title: 'tt_away', advanced: false },
  { field: 'auto_split_after_s', label: 'lb_auto_split', unit: 'u_away_min', title: 'tt_auto_split', advanced: false },
  { field: 'poll_interval_ms', label: 'lb_poll', unit: 'u_every_ms', title: 'tt_poll', advanced: true },
  { field: 'glance_max_s', label: 'lb_glance', unit: 'u_under_s', title: 'tt_glance', advanced: true },
  { field: 'transient_max_s', label: 'lb_transient', unit: 'u_under_s', title: 'tt_transient', advanced: true },
  { field: 'gap_after_s', label: 'lb_gap', unit: 'u_jump_s', title: 'tt_gap', advanced: true },
  { field: 'support_window_s', label: 'lb_support', unit: 'u_min', title: 'tt_support', advanced: true },
]

/** Long durations read better in minutes: stored in seconds, shown and edited as whole minutes. */
const IN_MINUTES: ReadonlySet<ThresholdField> = new Set(['auto_split_after_s', 'support_window_s'])
export const toDisplay = (f: ThresholdField, stored: number): number => (IN_MINUTES.has(f) ? Math.round(stored / 60) : stored)
export const fromDisplay = (f: ThresholdField, shown: number): number => (IN_MINUTES.has(f) ? shown * 60 : shown)

export const CONFIG_FIELDS = THRESHOLDS.map((x) => x.field).filter((f): f is ConfigField => f !== 'auto_split_after_s')

export type ThresholdRow = { field: ThresholdField; label: string; unit: string; value: number; title: string }

/** Visible rows, and the advanced ones (null while `▸ advanced` is collapsed). */
export function thresholdRows(
  values: Record<ThresholdField, number>,
  showAdvanced: boolean,
): { main: ThresholdRow[]; advanced: ThresholdRow[] | null } {
  const row = (s: Spec): ThresholdRow => ({
    field: s.field,
    label: t(s.label),
    unit: t(s.unit),
    value: toDisplay(s.field, values[s.field]),
    title: `${s.field} — ${t(s.title)}`, // the only place the raw key shows
  })
  return {
    main: THRESHOLDS.filter((s) => !s.advanced).map(row),
    advanced: showAdvanced ? THRESHOLDS.filter((s) => s.advanced).map(row) : null,
  }
}

export const advancedLabel = (open: boolean): string => `${open ? '▾' : '▸'} ${t('advanced')}`

const label = (f: ThresholdField) => t(THRESHOLDS.find((s) => s.field === f)?.label ?? 'lb_passive')
/** A label used mid-sentence ("… shorter than away after"): first letter lowered (tr-aware). */
const mid = (f: ThresholdField) => {
  const l = label(f)
  return l.charAt(0).toLocaleLowerCase(lang()) + l.slice(1)
}

/** Returns a one-line error in words (never a config key), or null when usable. */
export function validateThresholds(c: Config): string | null {
  for (const f of CONFIG_FIELDS) {
    const v = c[f]
    if (!Number.isInteger(v) || v <= 0) return t('err_whole', { f: label(f) })
  }
  if (c.poll_interval_ms < 250) return t('err_poll', { f: label('poll_interval_ms') })
  if (c.gap_after_s * 1000 <= c.poll_interval_ms) return t('err_gap', { f: label('gap_after_s'), p: mid('poll_interval_ms') })
  if (c.passive_after_s >= c.away_after_s) return t('err_shorter', { a: label('passive_after_s'), b: mid('away_after_s') })
  if (c.transient_max_s >= c.glance_max_s) return t('err_shorter', { a: label('transient_max_s'), b: mid('glance_max_s') })
  return null
}

/** Prefs.auto_split_after_s: at least 5 minutes, so a coffee never splits a session. */
export function validateAutoSplit(s: number): string | null {
  return Number.isInteger(s) && s >= 300 ? null : t('err_auto_split', { f: label('auto_split_after_s') })
}

/** `# Behaviour` toggles bound to prefs, in display order (§22: open at login first). */
export const BEHAVIOUR_PREFS: readonly { pref: 'launch_at_login' | 'auto_track'; label: Key; hint: Key }[] = [
  { pref: 'launch_at_login', label: 'launch_at_login', hint: 'launch_at_login_hint' },
  { pref: 'auto_track', label: 'auto_start', hint: 'auto_hint' },
]

export const withPref = (p: Prefs, key: 'launch_at_login' | 'auto_track', on: boolean): Prefs => ({ ...p, [key]: on })
