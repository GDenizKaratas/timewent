import { afterEach, describe, expect, it } from 'vitest'
import type { Row, SessionMeta, Status } from './types'
import { setLang, t } from './i18n'
import { MIN, seg, view } from './testkit'
import {
  isFreshState,
  justEnded,
  liveKey,
  autoHint,
  expandButton,
  capDetails,
  receiptParts,
  summarySentence,
  howLines,
  fromLayout,
  toLayout,
  inUseMs,
  inUseTitle,
  moreToggle,
  pillModel,
  rangeIsLive,
  rangeKey,
  rangeTabs,
  recentActivity,
  tabRange,
  trackButton,
  whereSummary,
} from './viewmodel'

const at = (d: number, h: number, m: number) => new Date(2026, 9, d, h, m).getTime()

const row = (key: string, ms: number, share: number): Row => ({
  key, label: key, category: 'code', ms, share, details: [], kind: 'context', breakdown: [],
})

describe('whereSummary', () => {
  afterEach(() => setLang('en'))
  const seven = view({
    total_ms: 40 * MIN,
    away_ms: 13 * MIN + 2_000,
    rows: [
      { ...row('a', 20 * MIN, 0.74), details: [{ detail: 'x.py', ms: 12 * MIN }] },
      row('b', 2 * MIN, 0.08), row('c', 1 * MIN, 0.04), row('d', 1 * MIN, 0.04),
      row('e', 1 * MIN, 0.04), row('f', 1 * MIN, 0.04), row('g', 1 * MIN, 0.02),
    ],
  })

  it('shows at most `limit` rows and counts the rest', () => {
    const s = whereSummary(seven, { limit: 5, showAll: false, liveKey: null }, 12)
    expect(s.rows.map((r) => r.key)).toEqual(['a', 'b', 'c', 'd', 'e'])
    expect(s.more).toBe(2)
  })

  it('labels the inline toggle: show all (+n) / show less', () => {
    expect(moreToggle(whereSummary(seven, { limit: 5, showAll: false, liveKey: null }, 12), false)).toBe('Show all (+2)')
    expect(moreToggle(whereSummary(seven, { limit: 5, showAll: true, liveKey: null }, 12), true)).toBe('Show less')
    expect(moreToggle(whereSummary(view({ rows: [row('a', 1, 1)] }), { limit: 5, showAll: false, liveKey: null }, 12), false)).toBeNull()
  })

  it('shows every row when asked', () => {
    const s = whereSummary(seven, { limit: 5, showAll: true, liveKey: null }, 12)
    expect(s.rows).toHaveLength(7)
    expect(s.more).toBe(0)
  })

  it('formats time, meter model and text fallback per row', () => {
    const [a] = whereSummary(seven, { limit: 5, showAll: false, liveKey: null }, 10).rows // the panel's 10-cell meter
    expect(a).toMatchObject({ label: 'a', time: '20:00', bar: '███████░░░', pct: '74%', live: false })
    expect(a?.details).toEqual([{ detail: 'x.py', time: '12m' }])
  })

  it('marks only the live context row', () => {
    const s = whereSummary(seven, { limit: 5, showAll: false, liveKey: 'c' }, 12)
    expect(s.rows.map((r) => r.live)).toEqual([false, false, true, false, false])
  })

  it('never shows passthrough (pass:*) contexts as rows, nor counts them in "more"', () => {
    const v = view({ rows: [row('code:a', 5 * MIN, 0.9), row('pass:dev.timewent.app', 10_000, 0.1)] })
    const s = whereSummary(v, { limit: 1, showAll: false, liveKey: null }, 12)
    expect(s.rows.map((r) => r.key)).toEqual(['code:a'])
    expect(s.more).toBe(0)
  })

  it('project rows: the expand shows the category breakdown first, translated', () => {
    const v = view({
      rows: [{
        ...row('code:bank-agent-lab', 65 * MIN, 0.9), label: 'bank-agent-lab', kind: 'project',
        breakdown: [{ key: 'code', label: 'code', ms: 38 * MIN }, { key: 'ai', label: 'ai', ms: 12 * MIN }],
      }],
    })
    const [r] = whereSummary(v, { limit: 5, showAll: false, liveKey: null }, 10).rows
    expect(r).toMatchObject({ kind: 'project', breakdown: 'code 38m · ai 12m', expandable: true })
    setLang('tr')
    expect(whereSummary(v, { limit: 5, showAll: false, liveKey: null }, 10).rows[0]?.breakdown).toBe('kod 38dk · yapay zeka 12dk')
  })

  it('activity rows: breakdown by member name (data, never translated)', () => {
    setLang('tr')
    const v = view({
      rows: [{
        ...row('act:coding', 48 * MIN, 1), label: 'coding', kind: 'activity',
        breakdown: [{ key: 'com.microsoft.VSCode', label: 'VS Code', ms: 38 * MIN }, { key: 'com.googlecode.iterm2', label: 'iTerm2', ms: 10 * MIN }],
        details: [{ detail: 'bank-agent-lab · risk_engine.py', ms: 30 * MIN }],
      }],
    })
    const [r] = whereSummary(v, { limit: 5, showAll: false, liveKey: null }, 10).rows
    expect(r?.breakdown).toBe('VS Code 38dk · iTerm2 10dk')
    expect(r?.details).toEqual([{ detail: 'bank-agent-lab · risk_engine.py', time: '30dk' }])
  })

  it('context rows have no breakdown line; rows with nothing to show do not expand', () => {
    const [r] = whereSummary(view({ rows: [row('web:x', MIN, 1)] }), { limit: 5, showAll: false, liveKey: null }, 10).rows
    expect(r).toMatchObject({ kind: 'context', breakdown: null, expandable: false })
  })

  it('never shows away as a row (it lives in the how block)', () => {
    const s = whereSummary(seven, { limit: 5, showAll: false, liveKey: null }, 12)
    expect(s.rows.some((r) => r.key === 'away')).toBe(false)
  })
})

describe('autoHint', () => {
  afterEach(() => setLang('en'))
  it('says when auto mode splits, from auto_split_after_s', () => {
    expect(autoHint(1800)).toBe('Starts on first input, splits after 30m away')
    expect(autoHint(3600)).toBe('Starts on first input, splits after 1h00m away')
    setLang('tr')
    expect(autoHint(1800)).toBe('İlk girişte başlar, 30dk uzaklıkta böler')
  })
})

describe('one duration grammar (DESIGN §9): bright = clock, dim = compact', () => {
  const CLOCK = /^\d+:\d\d(:\d\d)?$/
  const COMPACT = /^\d+(h\d\dm|m|s)$/
  const v = view({
    away_ms: 13 * MIN,
    longest_focus_ms: 21 * MIN,
    rows: [{
      ...row('code:p', 74 * MIN, 1), label: 'p', kind: 'project',
      breakdown: [{ key: 'code', label: 'code', ms: 51 * MIN }],
      details: [{ detail: 'risk_engine.py', ms: 32 * MIN + 21_000 }],
    }],
    segments: [seg({ start_ms: 0, end_ms: 16 * MIN, details: [{ detail: 'x.py', ms: 16 * MIN }] })],
    listening: [{ label: 'YouTube', title: 't', ms: 33 * MIN }],
  })
  it('primary: row time is a clock', () => {
    expect(whereSummary(v, { limit: 5, showAll: false, liveKey: null }, 10).rows[0]?.time).toMatch(CLOCK)
  })
  it('secondary: row details, breakdown, recent, ♪, away all compact', () => {
    const r = whereSummary(v, { limit: 5, showAll: false, liveKey: null }, 10).rows[0]
    expect(r?.details[0]?.time).toBe('32m')
    expect(r?.breakdown).toBe('code 51m')
    const [act] = recentActivity(v)
    expect(act?.howLong).toMatch(COMPACT)
    expect(act?.details[0]?.time).toMatch(COMPACT)
    for (const l of howLines(v).filter((x) => x.time)) expect(l.time).toMatch(COMPACT)
    expect(howLines(v).find((x) => x.kind === 'away')?.value).toBe('13m')
  })
})

describe('details cap (DESIGN §11.2)', () => {
  afterEach(() => setLang('en'))
  const d = Array.from({ length: 9 }, (_, i) => ({ detail: `f${i}`, time: '1m' }))
  it('shows 5, then "+n more" as an inline toggle; all when asked', () => {
    expect(capDetails(d, false)).toMatchObject({ shown: d.slice(0, 5), toggle: '+4 more' })
    expect(capDetails(d, true)).toMatchObject({ shown: d, toggle: 'Show less' })
    setLang('tr')
    expect(capDetails(d, false).toggle).toBe('+4 daha')
  })
  it('no toggle when it all fits', () => {
    expect(capDetails(d.slice(0, 5), false)).toEqual({ shown: d.slice(0, 5), toggle: null })
  })
})

describe('receiptParts (DESIGN §11.4 line 2)', () => {
  afterEach(() => setLang('en'))
  const v = view({
    active_ms: 70 * MIN, passive_ms: 11 * MIN, away_ms: 13 * MIN, longest_focus_ms: 21 * MIN,
    rows: [row('pass:dev.timewent.app', MIN, 0.01), { ...row('code:bank-agent-lab', 74 * MIN, 0.9), label: 'bank-agent-lab' }],
  })
  it('top row (clock-short) · away, as label/value parts — no in-use, no focus', () => {
    expect(receiptParts(v)).toEqual([{ name: 'bank-agent-lab', value: '1:14' }, { label: 'Away', value: '13m' }])
    setLang('tr')
    expect(receiptParts(v)).toEqual([{ name: 'bank-agent-lab', value: '1:14' }, { label: 'Uzakta', value: '13dk' }])
  })
  it('line 1 is short enough to sit beside [copy]', () => {
    for (const l of ['en', 'tr'] as const) {
      setLang(l)
      expect([...t('receipt_title')].length).toBeLessThanOrEqual(30)
    }
    expect(t('receipt_title')).toBe('Oturum bitti')
  })
  it('leaves out what is empty', () => {
    expect(receiptParts(view({ rows: [row('code:a', MIN, 1)] }))).toEqual([{ name: 'code:a', value: '0:01' }])
    expect(receiptParts(view())).toEqual([])
  })
})

describe('summarySentence (DESIGN §11.2, DESIGN §11.2: label dim / value bright, one line)', () => {
  afterEach(() => setLang('en'))
  const v = view({
    longest_focus_ms: 21 * MIN,
    switches: 4,
    rows: [row('pass:dev.timewent.app', 2 * MIN, 0.02), { ...row('code:bank-agent-lab', 74 * MIN, 0.9), label: 'bank-agent-lab' }],
  })
  it('parts as label/value spans: Most: <name> · longest focus <compact>', () => {
    expect(summarySentence(v)).toEqual({
      parts: [{ label: 'Most:', name: 'bank-agent-lab' }, { label: 'Longest focus', value: '21m' }],
      title: '4 switches',
    })
    setLang('tr')
    expect(summarySentence(v)?.parts).toEqual([{ label: 'En çok:', name: 'bank-agent-lab' }, { label: 'En uzun odak', value: '21dk' }])
  })
  it('the name is the shrinking part; durations are compact (3-part too)', () => {
    setLang('tr')
    const long = view({
      longest_focus_ms: 64 * MIN,
      rows: [{ ...row('act:x', 64 * MIN, 1), label: 'a very long activity name that will never fit on one line' }],
    })
    const [name, focus] = summarySentence(long)?.parts ?? []
    expect(name?.name).toMatch(/^a very long/)
    expect(focus?.value).toBe('1sa04dk')
  })
  it('skips passthrough rows; leaves out empty parts; nothing → null', () => {
    expect(summarySentence(view({ rows: [row('code:a', MIN, 1)] }))?.parts).toEqual([{ label: 'Most:', name: 'code:a' }])
    expect(summarySentence(view({ longest_focus_ms: 21 * MIN }))?.parts).toEqual([{ label: 'Longest focus', value: '21m' }])
    expect(summarySentence(view())).toBeNull()
  })
})

describe('howLines (DESIGN §11.2: away · ♪)', () => {
  afterEach(() => setLang('en'))
  const full = view({
    rows: [row('a', MIN, 1)],
    away_ms: 13 * MIN,
    longest_focus_ms: 21 * MIN,
    switches: 4,
    categories: [
      { category: 'code', ms: 63 * MIN, share: 0.63 }, { category: 'ai', ms: 20 * MIN, share: 0.2 },
      { category: 'web', ms: 10 * MIN, share: 0.1 }, { category: 'docs', ms: 6 * MIN, share: 0.06 },
      { category: 'notes', ms: MIN, share: 0.01 },
    ],
    listening: [
      { label: 'YouTube', title: 'lofi beats to code to', ms: 33 * MIN },
      { label: 'Spotify', title: null, ms: 10 * MIN },
      { label: 'Music', title: 'x', ms: MIN },
    ],
  })

  it('♪ lines keep the source visible: the title ellipsizes, the source never does', () => {
    const [yt, sp] = howLines(full).filter((l) => l.kind === 'listen')
    expect(yt).toMatchObject({ value: 'lofi beats to code to', source: 'YouTube', time: '33m' })
    expect(sp).toMatchObject({ value: 'Spotify', time: '10m' })
    expect(sp?.source).toBeUndefined()
  })

  it('away · ♪ (≤ 2) — focus moved to the summary sentence (DESIGN §11.2)', () => {
    expect(howLines(full).map((l) => [l.key, l.source ? `${l.value} · ${l.source}` : l.value, l.time ?? null])).toEqual([
      ['Away', '13m', null],
      ['♪', 'lofi beats to code to · YouTube', '33m'],
      ['♪', 'Spotify', '10m'],
    ])
  })

  it('turkish keys', () => {
    setLang('tr')
    expect(howLines(full).map((l) => l.key)).toEqual(['Uzakta', '♪', '♪'])
  })

  it('omits empty lines', () => {
    expect(howLines(view({ rows: [row('a', MIN, 1)] }))).toEqual([])
    expect(howLines(view({ away_ms: 60_000 })).map((l) => l.kind)).toEqual(['away'])
  })
})

describe('rangeIsLive (the tape\'s last cell glows green)', () => {
  const st: Status = {
    tracking: true, session_id: 9, started_at_ms: 0, elapsed_ms: 1, current: null, permissions: { accessibility: true }, auto: false,
  }
  it('only while tracking and the range contains the running session', () => {
    expect(rangeIsLive(st, { kind: 'session', id: 9 })).toBe(true)
    expect(rangeIsLive(st, { kind: 'today' })).toBe(true)
    expect(rangeIsLive(st, { kind: 'week' })).toBe(true)
    expect(rangeIsLive(st, { kind: 'session', id: 8 })).toBe(false)
    expect(rangeIsLive({ ...st, tracking: false }, { kind: 'today' })).toBe(false)
    expect(rangeIsLive(null, { kind: 'today' })).toBe(false)
  })
})

describe('liveKey', () => {
  const st: Status = {
    tracking: true, session_id: 9, started_at_ms: 0, elapsed_ms: 1,
    current: { key: 'code:a', label: 'a', project: null, detail: null, category: 'code', presence: 'active', context_ms: 1, since_ms: null },
    permissions: { accessibility: true }, auto: false,
  }
  it('is the current key when the range contains the running session', () => {
    expect(liveKey(st, { kind: 'session', id: 9 })).toBe('code:a')
    expect(liveKey(st, { kind: 'today' })).toBe('code:a')
    expect(liveKey(st, { kind: 'week' })).toBe('code:a')
  })
  it('is null for other sessions, when idle, or when away', () => {
    expect(liveKey(st, { kind: 'session', id: 8 })).toBeNull()
    expect(liveKey({ ...st, tracking: false }, { kind: 'today' })).toBeNull()
    expect(liveKey({ ...st, current: { ...st.current!, presence: 'away' } }, { kind: 'today' })).toBeNull()
  })
})

describe('recentActivity', () => {
  const segs = [
    seg({ start_ms: at(6, 14, 18), end_ms: at(6, 14, 24), key: 'web:docs', label: 'docs', category: 'docs', details: [{ detail: 'docs.rs', ms: 6 * MIN }] }),
    seg({ start_ms: at(6, 14, 24), end_ms: at(6, 14, 24) + 9_000, key: 'web:youtube.com', label: 'YouTube', kind: 'glance', category: 'web' }),
    seg({ start_ms: at(6, 14, 25), end_ms: at(6, 14, 41), label: 'bank-agent-lab', details: [{ detail: 'risk_engine.py', ms: 10 * MIN }, { detail: 'api.py', ms: 6 * MIN }], explain: ['absorbed youtube.com (1.7s) — under transient threshold 3s'] }),
  ]
  const items = recentActivity(view({ segments: segs }))

  it('lists latest first', () => {
    expect(items.map((i) => i.what)).toEqual(['bank-agent-lab / risk_engine.py', 'YouTube · Glance', 'docs / docs.rs'])
  })

  it('reads what → when → how long (row order, rev 3)', () => {
    expect(items[0]).toMatchObject({ what: 'bank-agent-lab / risk_engine.py', when: '14:25–14:41', howLong: '16m' })
    expect(items[1]).toMatchObject({ what: 'YouTube · Glance', when: '14:24–14:24', howLong: '9s' })
    expect(Object.keys(items[0]!).slice(0, 3)).toEqual(['what', 'when', 'howLong'])
  })

  it('renders explain lines as terminal comments', () => {
    expect(items[0]?.lines).toEqual(['# absorbed youtube.com (1.7s) — under transient threshold 3s'])
  })

  it('keeps the original chronological index for cross-linking with the tape', () => {
    expect(items.map((i) => i.index)).toEqual([2, 1, 0])
  })

  it('away as given; gaps read "not tracked"; only glances get a suffix', () => {
    const [gap, away] = recentActivity(
      view({
        segments: [
          seg({ start_ms: 0, end_ms: 4 * MIN, kind: 'away', label: 'away' }),
          seg({ start_ms: 4 * MIN, end_ms: 9 * MIN, kind: 'gap', label: 'gap' }),
        ],
      }),
    )
    expect(gap).toMatchObject({ what: 'Not tracked', tone: 'gap' }) // sleep, app closed, between sessions
    expect(away).toMatchObject({ what: 'away', tone: 'away' })
  })

  it('hides passthrough (pass:*) segments; indexes still point into view.segments', () => {
    const items = recentActivity(
      view({
        segments: [
          seg({ start_ms: 0, end_ms: 60_000, label: 'a' }),
          seg({ start_ms: 60_000, end_ms: 66_000, kind: 'glance', key: 'pass:dev.timewent.app', label: 'timewent' }),
          seg({ start_ms: 66_000, end_ms: 120_000, key: 'web:b', label: 'b' }),
        ],
      }),
    )
    expect(items.map((i) => [i.what, i.index])).toEqual([['b', 2], ['a', 0]])
  })

  it('tones focus by its dominant presence', () => {
    const [it0] = recentActivity(view({ segments: [seg({ start_ms: 0, end_ms: 10, active_ms: 2, passive_ms: 8 })] }))
    expect(it0?.tone).toBe('passive')
  })
})

describe('pillModel', () => {
  const now = at(6, 15, 0)
  const base: Status = {
    tracking: false, session_id: null, started_at_ms: null, elapsed_ms: 0, current: null,
    permissions: { accessibility: true }, auto: false,
  }
  const cur = (p: Partial<NonNullable<Status['current']>> = {}): NonNullable<Status['current']> => ({
    key: 'code:bank-agent-lab', label: 'bank-agent-lab', project: null, detail: 'risk_engine.py', category: 'code',
    presence: 'active', context_ms: (38 * 60 + 14) * 1000, since_ms: null, ...p,
  })
  const tracking = (c: Status['current'], ax = true): Status => ({
    ...base, tracking: true, session_id: 3, started_at_ms: 0, elapsed_ms: (3600 + 42 * 60 + 7) * 1000, current: c,
    permissions: { accessibility: ax },
  })
  const TOTAL = { label: 'Session', value: '1:42:07', title: 'Wall-clock time since start, away included' }

  describe('idle', () => {
    it('the brand on line 1 (never translated); line 2 invites you to start', () => {
      expect(pillModel(base, now)).toEqual({
        tone: 'idle', title: 'timewent', time: null, timeTitle: null, badge: null,
        lead: { text: 'space to start', kind: 'hint' }, total: null,
      })
      expect(pillModel(base, now, { todayInUseMs: 0 }).lead?.text).toBe('space to start')
    })
    it('the tagline is no longer in the pill', () => {
      expect(JSON.stringify(pillModel(base, now))).not.toMatch(/where your time went/)
    })
    it('line 2 shows today\'s in-use time when there is some', () => {
      expect(pillModel(base, now, { todayInUseMs: (3 * 3600 + 12 * 60) * 1000 })).toMatchObject({
        title: 'timewent',
        lead: { text: 'Today 3:12:00 in use', kind: 'hint' },
      })
    })
    it('just stopped: brand, then "ended · in use X"', () => {
      expect(pillModel(base, now, { ended: { in_use_ms: (3600 + 11 * 60 + 31) * 1000 }, todayInUseMs: 1 })).toEqual({
        tone: 'idle', title: 'timewent', time: null, timeTitle: null, badge: null,
        lead: { text: 'Ended · in use 1:11:31', kind: 'hint' }, total: null,
      })
    })
  })

  it('the ended summary and today are ignored while tracking', () => {
    expect(pillModel(tracking(cur()), now, { ended: { in_use_ms: 1 }, todayInUseMs: 1 }).title).toBe('bank-agent-lab')
  })

  it('tracking: context + its time (labelled by tooltip), then detail · total', () => {
    expect(pillModel(tracking(cur()), now)).toEqual({
      tone: 'active', title: 'bank-agent-lab', time: '38:14', timeTitle: 'On bank-agent-lab this session', badge: null,
      lead: { text: 'risk_engine.py', kind: 'detail' }, total: TOTAL,
    })
  })

  it('never says "timewent" while tracking', () => {
    expect(pillModel(tracking(cur()), now).title).not.toMatch(/timewent/)
  })

  it('passive keeps the context, tone amber', () => {
    expect(pillModel(tracking(cur({ presence: 'passive' })), now)).toMatchObject({ tone: 'passive', title: 'bank-agent-lab' })
  })

  it('away: "away · since HH:MM" and how long; line 2 just the total', () => {
    const since = at(6, 14, 48)
    expect(pillModel(tracking(cur({ presence: 'away', since_ms: since })), now)).toEqual({
      tone: 'away', title: 'Away · since 14:48', time: '12:00', timeTitle: 'Away for', badge: null, lead: null, total: TOTAL,
    })
  })

  it('away without a start time still reads as away', () => {
    expect(pillModel(tracking(cur({ presence: 'away' })), now)).toMatchObject({ title: 'Away', time: null })
  })

  it('tracking before the first sample', () => {
    expect(pillModel(tracking(null), now)).toEqual({ tone: 'active', title: '…', time: null, timeTitle: null, badge: null, lead: null, total: TOTAL })
  })

  describe('project / activity (current.project)', () => {
    it('line 1 = project, line 2 = the context label · total; line-1 time is context_ms', () => {
      expect(pillModel(tracking(cur({ label: 'ChatGPT', detail: 'Rolling VaR', project: 'bank-agent-lab' })), now)).toMatchObject({
        title: 'bank-agent-lab', time: '38:14', timeTitle: 'On bank-agent-lab this session',
        lead: { text: 'ChatGPT', kind: 'detail' }, total: TOTAL,
      })
    })
    it('an activity name works the same way', () => {
      expect(pillModel(tracking(cur({ label: 'iTerm2', project: 'coding' })), now)).toMatchObject({ title: 'coding', lead: { text: 'iTerm2' } })
    })
    it('project equal to the label: no repeat, falls back to the detail', () => {
      expect(pillModel(tracking(cur({ project: 'bank-agent-lab' })), now)).toMatchObject({
        title: 'bank-agent-lab', lead: { text: 'risk_engine.py', kind: 'detail' },
      })
    })
  })

  describe('in turkish', () => {
    afterEach(() => setLang('en'))
    it('same layout, translated chrome; data untouched', () => {
      setLang('tr')
      expect(pillModel(base, now)).toMatchObject({ title: 'timewent', lead: { text: 'space ile başlat' } }) // brand never translated
      expect(pillModel(tracking(cur()), now)).toMatchObject({
        title: 'bank-agent-lab', timeTitle: 'Bu oturumda bank-agent-lab üzerinde',
        total: { label: 'Oturum', value: '1:42:07', title: 'Başlangıçtan beri geçen süre, uzakta dahil' },
      })
      expect(pillModel(tracking(cur({ presence: 'away', since_ms: at(6, 14, 48) })), now).title).toBe('Uzakta · son giriş 14:48')
    })
  })

  describe('auto mode', () => {
    afterEach(() => setLang('en'))
    it('a dim badge right before the line-1 time while auto tracking', () => {
      expect(pillModel({ ...tracking(cur()), auto: true }, now).badge).toBe('Auto')
      expect(pillModel(tracking(cur()), now).badge).toBeNull()
      setLang('tr')
      expect(pillModel({ ...tracking(cur()), auto: true }, now).badge).toBe('Oto')
    })
    it('no badge while idle', () => {
      expect(pillModel({ ...base, auto: true }, now).badge).toBeNull()
    })
  })

  describe('accessibility off', () => {
    // short enough to fit beside the total at 340px; the full reason lives in the tooltip
    const NUDGE = { text: 'Allow accessibility ↗', kind: 'ax' }
    it('the nudge replaces the detail, never the total', () => {
      expect(pillModel(tracking(cur(), false), now)).toMatchObject({ lead: NUDGE, total: TOTAL })
    })
    it('also when there is no detail, and when away', () => {
      expect(pillModel(tracking(cur({ detail: null }), false), now).lead).toEqual(NUDGE)
      expect(pillModel(tracking(cur({ presence: 'away', since_ms: now }), false), now).lead).toEqual(NUDGE)
    })
    it('not while idle (nothing is being read)', () => {
      expect(pillModel({ ...base, permissions: { accessibility: false } }, now).lead?.kind).toBe('hint')
    })
  })

  describe('line 2 never repeats line 1', () => {
    const lead = (detail: string | null, label = 'bank-agent-lab') => pillModel(tracking(cur({ label, detail })), now).lead
    it('no detail', () => expect(lead(null)).toBeNull())
    it('detail equals label', () => expect(lead('bank-agent-lab')).toBeNull())
    it('detail contains label (title starting with the project)', () => expect(lead('bank-agent-lab — Pull request #42')).toBeNull())
    it('containment ignores case and surrounding space', () => expect(lead('  Bank-Agent-Lab ')).toBeNull())
    it('blank detail', () => expect(lead('   ')).toBeNull())
    it('a distinct detail is kept', () => expect(lead('api.py')).toEqual({ text: 'api.py', kind: 'detail' }))
  })
})

describe('trackButton', () => {
  it('is a green primary ▶ when idle', () => {
    expect(trackButton(false)).toEqual({ text: '▶', title: 'Start (space)', kind: 'primary' })
  })
  it('is a neutral stop square when tracking', () => {
    expect(trackButton(true)).toEqual({ text: '■', title: 'Stop (space)', kind: 'neutral' })
  })
})

describe('rangeKey', () => {
  it('names each range (used in export file names)', () => {
    expect(rangeKey({ kind: 'today' })).toBe('today')
    expect(rangeKey({ kind: 'week' })).toBe('week')
    expect(rangeKey({ kind: 'session', id: 12 })).toBe('session-12')
  })
})

describe('layout ↔ mode (set_layout / peek contract)', () => {
  it('maps both ways', () => {
    expect(toLayout('collapsed')).toBe('pill')
    expect(toLayout('expanded')).toBe('expanded')
    expect(fromLayout('pill')).toBe('collapsed')
    expect(fromLayout('expanded')).toBe('expanded')
  })
})

describe('expandButton', () => {
  it('▾ expands, ▴ collapses (centred glyphs; ⌄/⌃ sit off-centre), with shortcut tooltips', () => {
    expect(expandButton(false)).toEqual({ text: '▾', title: 'Expand (↓)' })
    expect(expandButton(true)).toEqual({ text: '▴', title: 'Collapse (esc) — esc again hides' })
  })
})

describe('rangeTabs', () => {
  it('session · today · week, the effective range selected', () => {
    expect(rangeTabs({ kind: 'session', id: 9 }, true)).toEqual([
      { id: 'session', text: 'Session', title: 'This / last session (1)', selected: true, disabled: false },
      { id: 'today', text: 'Today', title: 'Today (2)', selected: false, disabled: false },
      { id: 'week', text: 'Week', title: 'Since Monday (3)', selected: false, disabled: false },
    ])
    expect(rangeTabs({ kind: 'week' }, true).map((t) => t.selected)).toEqual([false, false, true])
  })
  it('none selected while viewing a past session (DESIGN §11.5)', () => {
    expect(rangeTabs({ kind: 'session', id: 8 }, true, true).map((t) => t.selected)).toEqual([false, false, false])
  })
  it('disables session before any session exists', () => {
    expect(rangeTabs({ kind: 'today' }, false)[0]).toMatchObject({ selected: false, disabled: true })
  })
})

describe('tabRange', () => {
  it('session follows the latest session (null), the others are fixed ranges', () => {
    expect(tabRange('session')).toBeNull()
    expect(tabRange('today')).toEqual({ kind: 'today' })
    expect(tabRange('week')).toEqual({ kind: 'week' })
  })
})

describe('in use', () => {
  afterEach(() => setLang('en'))
  const v = view({
    total_ms: (58 * 60 + 20 + 13 * 60 + 11 + 13 * 60 + 2) * 1000,
    active_ms: (58 * 60 + 20) * 1000,
    passive_ms: (13 * 60 + 11) * 1000,
    away_ms: (13 * 60 + 2) * 1000,
  })
  it('counts active + passive, never away or gaps', () => {
    expect(inUseMs(v)).toBe((1 * 3600 + 11 * 60 + 31) * 1000)
  })
  it('explains itself in the tooltip', () => {
    expect(inUseTitle(v)).toBe('Active 58:20 + reading 13:11 · away 13:02 not counted')
  })
  it('drops the away clause when there was none', () => {
    expect(inUseTitle(view({ active_ms: 60_000, passive_ms: 0 }))).toBe('Active 1:00 + reading 0:00')
  })
  it('names uncredited passthrough time on its own line, all entries; omitted when empty', () => {
    const ns = view({ ...v, not_shown: [{ label: 'timewent', ms: 95_000 }, { label: 'Raycast', ms: 12_000 }] })
    expect(inUseTitle(ns).split('\n').pop()).toBe('Not shown: timewent 1m35s · Raycast 12s')
    setLang('tr')
    expect(inUseTitle(ns).split('\n').pop()).toBe('Gösterilmeyen: timewent 1dk 35sn · Raycast 12sn')
    setLang('en')
    expect(inUseTitle(v)).not.toMatch(/Not shown/)
  })

  it('then every kind of time with its share, on its own line (the panel no longer shows them)', () => {
    const withKinds = view({
      ...v,
      categories: [
        { category: 'code', ms: 1, share: 0.63 }, { category: 'ai', ms: 1, share: 0.2 }, { category: 'web', ms: 1, share: 0.1 },
        { category: 'docs', ms: 1, share: 0.06 }, { category: 'notes', ms: 1, share: 0.01 },
      ],
    })
    expect(inUseTitle(withKinds)).toBe('Active 58:20 + reading 13:11 · away 13:02 not counted\ncode 63% · ai 20% · web 10% · docs 6% · notes 1%')
    setLang('tr')
    expect(inUseTitle(withKinds).split('\n')[1]).toBe('kod 63% · yapay zeka 20% · web 10% · doküman 6% · notlar 1%')
  })
})

describe('panel notices', () => {
  const idle: Status = {
    tracking: false, session_id: null, started_at_ms: null, elapsed_ms: 0, current: null,
    permissions: { accessibility: true }, auto: false,
  }
  const now = at(6, 15, 0)
  const ended: SessionMeta[] = [{ id: 9, started_at_ms: at(6, 14, 0), ended_at_ms: at(6, 14, 58) }]

  it('shows the start prompt when idle and the range is empty', () => {
    expect(isFreshState(idle, view())).toBe(true)
    expect(isFreshState(idle, view({ segments: [seg({ start_ms: 0, end_ms: 1 })] }))).toBe(false)
    expect(isFreshState({ ...idle, tracking: true }, view())).toBe(false)
  })

  it('announces a just-ended session while viewing it', () => {
    expect(justEnded(idle, ended, { kind: 'session', id: 9 }, now)).toBe(true)
  })

  it('stays quiet while tracking, for other ranges, for open or old sessions', () => {
    expect(justEnded({ ...idle, tracking: true }, ended, { kind: 'session', id: 9 }, now)).toBe(false)
    expect(justEnded(idle, ended, { kind: 'today' }, now)).toBe(false)
    expect(justEnded(idle, [{ ...ended[0]!, ended_at_ms: null }], { kind: 'session', id: 9 }, now)).toBe(false)
    expect(justEnded(idle, ended, { kind: 'session', id: 9 }, now + 20 * MIN)).toBe(false)
  })
})
