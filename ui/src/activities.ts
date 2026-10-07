// §13.1 activities: pure helpers for the settings list and the edit view.
import { formatCompact } from './format'
import { t } from './i18n'
import type { Activity, SeenSources } from './types'

type Group = 'apps' | 'domains'

/** List line: member names (from what was seen), apps then sites; raw id when unseen. */
export function memberSummary(a: Activity, seen: SeenSources): string {
  const appName = (id: string) => seen.apps.find((x) => x.bundle_id === id)?.name ?? id
  const siteName = (d: string) => seen.domains.find((x) => x.domain === d)?.label ?? d
  const names = [...a.apps.map(appName), ...a.domains.map(siteName)]
  return names.length ? names.join(' · ') : t('act_no_members')
}

export type CheckItem = { id: string; label: string; time: string; checked: boolean }
export type Checklist = { apps: CheckItem[]; sites: CheckItem[]; hidden: number }

/** Edit view: apps, then sites, ms desc, top `limit` per group unless `showAll`. Checked members
 *  always show (also ones not seen in the last 30 days). */
export function checklist(seen: SeenSources, draft: Activity, showAll: boolean, limit: number): Checklist {
  let hidden = 0
  const group = (items: { id: string; label: string; ms: number }[], selected: string[]) => {
    const all = items.map((i) => ({ id: i.id, label: i.label, time: formatCompact(i.ms), checked: selected.includes(i.id) }))
    const unseen = selected.filter((id) => !items.some((i) => i.id === id)).map((id) => ({ id, label: id, time: '—', checked: true }))
    const shown = showAll ? all : all.filter((x, i) => i < limit || x.checked)
    hidden += all.length - shown.length
    return [...shown, ...unseen]
  }
  const apps = group(seen.apps.map((a) => ({ id: a.bundle_id, label: a.name, ms: a.ms })), draft.apps)
  const sites = group(seen.domains.map((d) => ({ id: d.domain, label: d.label, ms: d.ms })), draft.domains)
  return { apps, sites, hidden }
}

export function toggleMember(a: Activity, group: Group, id: string): Activity {
  const list = a[group]
  return { ...a, [group]: list.includes(id) ? list.filter((x) => x !== id) : [...list, id] }
}

/** New ones go last; an edited one keeps its place (list order = match priority). */
export function upsertActivity(list: readonly Activity[], prevName: string | null, a: Activity): Activity[] {
  const clean = { ...a, name: a.name.trim() }
  const i = prevName === null ? -1 : list.findIndex((x) => x.name === prevName)
  return i < 0 ? [...list, clean] : list.map((x, j) => (j === i ? clean : x))
}

export const removeActivity = (list: readonly Activity[], name: string): Activity[] => list.filter((a) => a.name !== name)

/** One-line error, or null. */
export function validateActivity(a: Activity, list: readonly Activity[], editing: string | null): string | null {
  const name = a.name.trim().toLowerCase()
  if (!name) return t('act_err_name')
  if (a.apps.length + a.domains.length === 0) return t('act_err_members')
  if (list.some((x) => x.name.trim().toLowerCase() === name && x.name !== editing)) return t('act_err_dupe')
  return null
}
