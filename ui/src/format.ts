// Duration and clock formatting. All durations are ms in, floored to whole seconds.
import { t } from './i18n'

const pad2 = (n: number) => String(n).padStart(2, '0')

function hms(ms: number): [number, number, number] {
  const total = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : 0
  return [Math.floor(total / 3600), Math.floor((total % 3600) / 60), total % 60]
}

/** `m:ss` under an hour, `h:mm:ss` from an hour on. */
export function formatDuration(ms: number): string {
  const [h, m, s] = hms(ms)
  return h > 0 ? `${h}:${pad2(m)}:${pad2(s)}` : `${m}:${pad2(s)}`
}

/** Short clock `h:mm` (receipts): 1:14, 0:42. */
export function formatClockShort(ms: number): string {
  const [h, m] = hms(ms)
  return `${h}:${pad2(m)}`
}

/** Compact form for dense lists, in the current language: `9s` `16m` `1h04m` / `9sn` `16dk` `1sa04dk`. */
export function formatCompact(ms: number): string {
  const [h, m, s] = hms(ms)
  if (h > 0) return `${h}${t('unit_h')}${pad2(m)}${t('unit_m')}`
  if (m > 0) return `${m}${t('unit_m')}`
  return `${s}${t('unit_s')}`
}

/** Local wall-clock `HH:MM`. */
export function clockTime(ms: number): string {
  const d = new Date(ms)
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`
}

/** Local `HH:MM–HH:MM` (en dash). */
export function clockRange(startMs: number, endMs: number): string {
  return `${clockTime(startMs)}–${clockTime(endMs)}`
}
