import { afterEach, describe, expect, it } from 'vitest'
import { setLang, t } from './i18n'
import { advancedLabel, BEHAVIOUR_PREFS, fromDisplay, withPref, thresholdRows, toDisplay, validateAutoSplit, validateThresholds } from './settings-model'
import type { Config, Prefs } from './types'

const ok: Config = {
  poll_interval_ms: 1000, passive_after_s: 45, away_after_s: 180, gap_after_s: 5,
  transient_max_s: 3, glance_max_s: 10, labels: {}, docs_domains: [], passthrough_bundle_ids: [], count_self: false,
  support_window_s: 600, attribute_projects: true, activities: [], app_categories: {},
}

describe('validateThresholds (errors speak in labels, not config keys)', () => {
  afterEach(() => setLang('en'))
  it('accepts the plan defaults', () => {
    expect(validateThresholds(ok)).toBeNull()
  })
  it('rejects non-positive or fractional values, naming the field by its label', () => {
    expect(validateThresholds({ ...ok, away_after_s: 0 })).toBe('Away after: use a whole number above 0')
    expect(validateThresholds({ ...ok, gap_after_s: 2.5 })).toMatch(/^Not tracked after:/)
    expect(validateThresholds({ ...ok, glance_max_s: Number.NaN })).toMatch(/^Glance under:/)
    expect(validateThresholds({ ...ok, support_window_s: 0 })).toMatch(/^Research window:/)
  })
  it('requires passive < away', () => {
    expect(validateThresholds({ ...ok, passive_after_s: 200 })).toBe('Passive after must be shorter than away after')
  })
  it('requires absorb < glance', () => {
    expect(validateThresholds({ ...ok, transient_max_s: 10 })).toBe('Absorb under must be shorter than glance under')
  })
  it('requires the gap threshold to exceed one sample, or every sample would be a gap', () => {
    expect(validateThresholds({ ...ok, poll_interval_ms: 5000 })).toMatch(/Not tracked after/)
  })
  it('keeps sampling at or above 250 ms to stay ~0% cpu', () => {
    expect(validateThresholds({ ...ok, poll_interval_ms: 100 })).toMatch(/Sample every/)
  })
  it('never shows a raw key, in either language', () => {
    for (const l of ['en', 'tr'] as const) {
      setLang(l)
      for (const bad of [{ away_after_s: 0 }, { passive_after_s: 200 }, { transient_max_s: 10 }, { poll_interval_ms: 5000 }, { poll_interval_ms: 100 }]) {
        expect(validateThresholds({ ...ok, ...bad })).not.toMatch(/_/)
      }
      expect(validateAutoSplit(299)).not.toMatch(/_/)
    }
  })
})

describe('validateAutoSplit', () => {
  afterEach(() => setLang('en'))
  it('auto mode splits sessions after at least 5 minutes away', () => {
    expect(validateAutoSplit(1800)).toBeNull()
    expect(validateAutoSplit(300)).toBeNull()
    expect(validateAutoSplit(299)).toBe('Auto split after: at least 5 min')
    expect(validateAutoSplit(Number.NaN)).toMatch(/^Auto split after/)
    setLang('tr')
    expect(validateAutoSplit(240)).toBe('Oto bölme: en az 5 dk')
  })
})

describe('display units (DESIGN §11.6 fix: long durations in minutes)', () => {
  it('auto split and research window: minutes on screen, seconds in storage', () => {
    expect(toDisplay('auto_split_after_s', 1800)).toBe(30)
    expect(toDisplay('support_window_s', 600)).toBe(10)
    expect(fromDisplay('auto_split_after_s', 30)).toBe(1800)
    expect(fromDisplay('support_window_s', 10)).toBe(600)
  })
  it('odd seconds round to the nearest whole minute on screen', () => {
    expect(toDisplay('support_window_s', 650)).toBe(11)
  })
  it('the rest are shown as stored (s, ms)', () => {
    expect(toDisplay('passive_after_s', 45)).toBe(45)
    expect(fromDisplay('poll_interval_ms', 1000)).toBe(1000)
  })
})

describe('threshold rows (DESIGN §11.6: human labels, units in words, raw key only in the tooltip)', () => {
  afterEach(() => setLang('en'))
  const values = { ...ok, auto_split_after_s: 1800 }

  it('visible: passive after, away after, auto split after — the rest under ▸ advanced', () => {
    const r = thresholdRows(values, false)
    expect(r.main.map((x) => [x.label, x.value, x.unit])).toEqual([
      ['Passive after', 45, 's'],
      ['Away after', 180, 's'],
      ['Auto split after', 30, 'min away'], // shown in minutes, stored in seconds
    ])
    expect(r.advanced).toBeNull() // collapsed
    expect(advancedLabel(false)).toBe('▸ Advanced')
  })

  it('advanced, when open: sample period, glance, absorb, gap, research window', () => {
    const r = thresholdRows(values, true)
    expect(r.advanced?.map((x) => x.field)).toEqual([
      'poll_interval_ms', 'glance_max_s', 'transient_max_s', 'gap_after_s', 'support_window_s',
    ])
    expect(r.advanced?.[0]).toMatchObject({ label: 'Sample every', unit: 'ms', value: 1000 })
    expect(advancedLabel(true)).toBe('▾ Advanced')
  })

  it('the raw config key appears only in the tooltip, with the full meaning', () => {
    const [passive] = thresholdRows(values, false).main
    expect(passive?.label).not.toMatch(/_/)
    expect(passive?.title).toBe('passive_after_s — No input this long = passive (reading / thinking)')
  })

  it('turkish reads as a sentence: label · value · unit', () => {
    setLang('tr')
    const r = thresholdRows(values, true)
    expect(r.main.map((x) => `${x.label} ${x.value} ${x.unit}`)).toEqual([
      'Pasif sayılır 45 sn sonra', 'Uzakta sayılır 180 sn sonra', 'Oto bölme 30 dk uzaklıkta',
    ])
    expect(r.advanced?.find((x) => x.field === 'support_window_s')).toMatchObject({ value: 10, unit: 'dk' })
    expect(advancedLabel(false)).toBe('▸ Gelişmiş')
  })

  it('labels fit the 17ch column and units the ~17ch unit column, both languages', () => {
    for (const l of ['en', 'tr'] as const) {
      setLang(l)
      const r = thresholdRows(values, true)
      for (const x of [...r.main, ...(r.advanced ?? [])]) {
        expect([...x.label].length, `${l} ${x.field}`).toBeLessThanOrEqual(17)
        expect([...x.unit].length, `${l} ${x.field}`).toBeLessThanOrEqual(17)
      }
    }
  })
})

describe('behaviour toggles (DESIGN §11.8 open at login)', () => {
  const prefs: Prefs = {
    always_on_top: true, peek_shortcut: 'Alt+Shift+Space', auto_track: false, auto_split_after_s: 1800, language: 'system',
    launch_at_login: true,
  }
  it('open at login comes first in # Behaviour, then auto start', () => {
    expect(BEHAVIOUR_PREFS.map((b) => b.pref)).toEqual(['launch_at_login', 'auto_track'])
  })
  it('a toggle writes only its own pref and keeps everything else', () => {
    expect(withPref(prefs, 'launch_at_login', false)).toEqual({ ...prefs, launch_at_login: false })
    expect(prefs.launch_at_login).toBe(true) // pure
  })
  it('labels and hints are sentences in both languages', () => {
    for (const l of ['en', 'tr'] as const) {
      setLang(l)
      const b = BEHAVIOUR_PREFS[0]!
      expect(t(b.label)).toBe(l === 'en' ? 'Open at login' : 'Girişte aç')
      expect(t(b.hint)).toBe(l === 'en' ? 'Starts with your Mac, hidden as the pill' : 'Mac açılınca pill olarak başlar')
    }
    setLang('en')
  })
})
