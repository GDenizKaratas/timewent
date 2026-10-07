import { describe, expect, it } from 'vitest'
import { createMockBackend } from './mock'
import { pinLanguages } from './test-setup'
import type { View } from './types'

function clock(start = new Date(2026, 9, 6, 15, 0).getTime()) {
  let t = start
  return { now: () => t, advance: (ms: number) => (t += ms) }
}

function expectConsistent(v: View) {
  const span = (k: string) =>
    v.segments.filter((s) => s.kind === k).reduce((a, s) => a + s.end_ms - s.start_ms, 0)
  // passthrough (pass:*) time counts toward in-use (DESIGN §8.5) but never gets a row
  const nonGap = v.segments.filter((s) => s.kind !== 'gap').reduce((a, s) => a + s.end_ms - s.start_ms, 0)
  const pass = v.segments.filter((s) => s.key.startsWith('pass:')).reduce((a, s) => a + s.end_ms - s.start_ms, 0)
  expect(v.total_ms).toBe(nonGap)
  expect(v.away_ms).toBe(span('away'))
  expect(v.active_ms + v.passive_ms + v.away_ms).toBe(v.total_ms)
  expect(v.rows.reduce((a, r) => a + r.ms, 0)).toBe(v.total_ms - v.away_ms - pass)
  // shares use the header's denominator: in use = active + passive
  const inUse = v.active_ms + v.passive_ms
  expect(v.rows.reduce((a, r) => a + r.share, 0)).toBeCloseTo(inUse > 0 ? (inUse - pass) / inUse : 0, 6)
  for (let i = 1; i < v.segments.length; i++) {
    expect(v.segments[i]?.start_ms).toBe(v.segments[i - 1]?.end_ms)
  }
  for (const r of v.rows) {
    expect(r.details.reduce((a, d) => a + d.ms, 0)).toBeLessThanOrEqual(r.ms)
  }
  const ms = v.rows.map((r) => r.ms)
  expect(ms).toEqual([...ms].sort((a, b) => b - a))
}

describe('mock backend', () => {
  it('starts mid-session with realistic, internally consistent data', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now })
    const st = await api.get_status()
    expect(st.tracking).toBe(true)
    expect(st.elapsed_ms).toBeGreaterThan(60 * 60_000)
    const v = await api.get_view({ kind: 'session', id: st.session_id ?? -1 })
    expectConsistent(v)
    const kinds = new Set(v.segments.map((s) => s.kind))
    expect([...kinds].sort()).toEqual(['away', 'focus', 'gap', 'glance'])
    expect(v.segments.some((s) => s.explain.some((l) => l.startsWith('absorbed ')))).toBe(true)
    expect(v.segments.some((s) => s.interruptions.length > 0)).toBe(true)
  })

  it('ticks: elapsed grows and the current context changes over time', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now })
    const a = await api.get_status()
    const seen = new Set<string>()
    for (let i = 0; i < 300; i++) {
      c.advance(1000)
      seen.add((await api.get_status()).current?.label ?? '')
    }
    const b = await api.get_status()
    expect(b.elapsed_ms - a.elapsed_ms).toBe(300_000)
    expect(seen.size).toBeGreaterThan(1)
  })

  it('the session view ends at the current time', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now })
    const st = await api.get_status()
    const v = await api.get_view({ kind: 'session', id: st.session_id ?? -1 })
    expect(v.segments[v.segments.length - 1]?.end_ms).toBe(c.now())
    expect(v.segments[0]?.start_ms).toBe(st.started_at_ms)
  })

  it('stop then start opens a fresh session', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now })
    const first = await api.get_status()
    const stopped = await api.stop_session()
    expect(stopped).toMatchObject({ tracking: false, session_id: null, current: null, elapsed_ms: 0 })
    c.advance(60_000)
    const started = await api.start_session()
    expect(started.tracking).toBe(true)
    expect(started.session_id).not.toBe(first.session_id)
    c.advance(90_000)
    const v = await api.get_view({ kind: 'session', id: started.session_id ?? -1 })
    expectConsistent(v)
    expect(v.total_ms).toBe(90_000)
    const sessions = await api.list_sessions(10)
    expect(sessions[0]).toMatchObject({ id: started.session_id, ended_at_ms: null })
    expect(sessions.find((s) => s.id === first.session_id)?.ended_at_ms).toBe(c.now() - 150_000)
  })

  it('today spans several sessions with a gap between them', async () => {
    const api = createMockBackend({ now: clock().now })
    const v = await api.get_view({ kind: 'today' })
    expectConsistent(v)
    expect(v.range).toEqual({ kind: 'today' })
    const session = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expect(v.total_ms).toBeGreaterThan(session.total_ms)
  })

  it('every listed session has a consistent view', async () => {
    const api = createMockBackend({ now: clock().now })
    for (const s of await api.list_sessions(10)) expectConsistent(await api.get_view({ kind: 'session', id: s.id }))
  })

  it('stores config', async () => {
    const api = createMockBackend({ now: clock().now })
    const cfg = await api.get_config()
    expect(cfg.passive_after_s).toBe(45)
    const saved = await api.set_config({ ...cfg, passive_after_s: 60 })
    expect(saved.passive_after_s).toBe(60)
    expect((await api.get_config()).passive_after_s).toBe(60)
  })

  it('export returns the written path', async () => {
    const api = createMockBackend({ now: clock().now })
    expect(await api.export_json({ kind: 'today' }, '/tmp/x.json')).toBe('/tmp/x.json')
  })

  it('fresh scenario: never started, nothing to show, start works', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now, scenario: 'fresh' })
    expect(await api.get_status()).toMatchObject({ tracking: false, session_id: null })
    expect(await api.list_sessions(10)).toEqual([])
    expect((await api.get_view({ kind: 'today' })).segments).toEqual([])
    const st = await api.start_session()
    c.advance(5_000)
    expect((await api.get_view({ kind: 'session', id: st.session_id ?? -1 })).total_ms).toBe(5_000)
  })

  it('stopped scenario: the latest session ended moments ago', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now, scenario: 'stopped' })
    expect((await api.get_status()).tracking).toBe(false)
    const [latest] = await api.list_sessions(1)
    expect(latest?.ended_at_ms).toBe(c.now() - 30_000)
    const v = await api.get_view({ kind: 'session', id: latest?.id ?? -1 })
    expectConsistent(v)
    expect(v.segments[v.segments.length - 1]?.end_ms).toBe(latest?.ended_at_ms)
  })
})

describe('mock get_info', () => {
  it('returns a data path and version', async () => {
    const info = await createMockBackend({ now: () => 0 }).get_info()
    expect(info.data_path).toMatch(/timewent\.db/)
    expect(info.version).toMatch(/^\d+\.\d+\.\d+/)
  })
  it('labels away and gap segments "away" / "gap" with no category, per DESIGN §11.10 notes', async () => {
    const api = createMockBackend({ now: () => new Date(2026, 9, 6, 15, 0).getTime() })
    const v = await api.get_view({ kind: 'today' })
    for (const s of v.segments.filter((x) => x.kind === 'away' || x.kind === 'gap')) {
      expect(s.label).toBe(s.kind)
      expect(s.category).toBeNull()
    }
  })
})

describe('mock status.current (DESIGN §11.1 contract)', () => {
  it('context_ms equals that context row in the session view', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now })
    for (let i = 0; i < 5; i++) {
      c.advance(37_000)
      const st = await api.get_status()
      const v = await api.get_view({ kind: 'session', id: st.session_id ?? -1 })
      const cur = st.current
      expect(cur).not.toBeNull()
      expect(cur?.context_ms).toBe(v.rows.find((r) => r.key === cur?.key)?.ms)
      expect(cur?.label).not.toMatch(/timewent/)
    }
  })

  it('away scenario: presence away with since_ms at the start of the away stretch', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now, scenario: 'away' })
    const st = await api.get_status()
    expect(st.tracking).toBe(true)
    expect(st.current?.presence).toBe('away')
    const v = await api.get_view({ kind: 'session', id: st.session_id ?? -1 })
    const last = v.segments[v.segments.length - 1]
    expect(last?.kind).toBe('away')
    expect(st.current?.since_ms).toBe(last?.start_ms)
  })

  it('config carries passthrough bundle ids, timewent itself first', async () => {
    const cfg = await createMockBackend({ now: clock().now }).get_config()
    expect(cfg.passthrough_bundle_ids[0]).toBe('dev.timewent.app')
  })

  it('hide_window resolves', async () => {
    await expect(createMockBackend({ now: clock().now }).hide_window()).resolves.toBeUndefined()
  })
})

describe('mock prefs', () => {
  it('keeps the window on top by default and stores the toggle', async () => {
    const api = createMockBackend({ now: clock().now })
    expect(await api.get_prefs()).toEqual({
      always_on_top: true, peek_shortcut: 'Alt+Shift+Space', auto_track: false, auto_split_after_s: 1800, language: 'system',
      launch_at_login: true, // DESIGN §11.8: on by default
    })
    const next = {
      always_on_top: false, peek_shortcut: 'Ctrl+Alt+T', auto_track: true, auto_split_after_s: 900, language: 'tr' as const,
      launch_at_login: false,
    }
    expect(await api.set_prefs(next)).toEqual(next)
    expect(await api.get_prefs()).toEqual(next)
  })
  it('rejects a peek key another app owns and keeps the old one', async () => {
    const api = createMockBackend({ now: clock().now })
    await expect(api.set_prefs({ ...(await api.get_prefs()), peek_shortcut: 'Cmd+Space' })).rejects.toThrow(/taken/)
    expect((await api.get_prefs()).peek_shortcut).toBe('Alt+Shift+Space')
  })
})

describe('mock week', () => {
  it('spans monday 00:00 → now, several days, each day separated by a gap', async () => {
    const c = clock(new Date(2026, 9, 10, 15, 0).getTime()) // a saturday
    const api = createMockBackend({ now: c.now })
    const week = await api.get_view({ kind: 'week' })
    expectConsistent(week)
    expect(week.range).toEqual({ kind: 'week' })
    const day = (ms: number) => new Date(ms).toDateString()
    const days = new Set(week.segments.filter((s) => s.kind !== 'gap').map((s) => day(s.start_ms)))
    expect(days.size).toBeGreaterThanOrEqual(4)
    for (let i = 1; i < week.segments.length; i++) {
      const a = week.segments[i - 1]!
      const b = week.segments[i]!
      if (a.kind !== 'gap' && b.kind !== 'gap') expect(day(a.start_ms)).toBe(day(b.start_ms))
    }
    const today = await api.get_view({ kind: 'today' })
    expect(week.total_ms).toBeGreaterThan(today.total_ms)
    const monday = new Date(2026, 9, 5).getTime()
    expect(week.segments[0]!.start_ms).toBeGreaterThanOrEqual(monday)
    expect(week.segments[0]!.start_ms).toBeLessThan(monday + 86_400_000) // monday has data
  })
})

describe('mock count_self', () => {
  it('off by default: timewent is a pass: segment — in use, never a row', async () => {
    const api = createMockBackend({ now: clock().now })
    expect((await api.get_config()).count_self).toBe(false)
    const v = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expect(v.segments.some((s) => s.key.startsWith('pass:'))).toBe(true)
    expect(v.rows.some((r) => r.key.startsWith('pass:') || r.label === 'timewent')).toBe(false)
  })
  it('on: timewent becomes an ordinary counted context', async () => {
    const api = createMockBackend({ now: clock().now })
    await api.set_config({ ...(await api.get_config()), count_self: true })
    const v = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expectConsistent(v)
    expect(v.segments.some((s) => s.key.startsWith('pass:'))).toBe(false)
    expect(v.rows.some((r) => r.label === 'timewent')).toBe(true)
  })
})

describe('mock quit_app', () => {
  it('resolves (the real one ends the session and exits)', async () => {
    await expect(createMockBackend({ now: clock().now }).quit_app()).resolves.toBeUndefined()
  })
})

describe('mock set_layout', () => {
  it('accepts both layouts', async () => {
    const api = createMockBackend({ now: clock().now })
    await expect(api.set_layout('expanded')).resolves.toBeUndefined()
    await expect(api.set_layout('pill')).resolves.toBeUndefined()
  })
})

describe('mock idle scenario', () => {
  it('not tracking, last session ended 20 minutes ago, today has in-use time', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now, scenario: 'idle' })
    expect((await api.get_status()).tracking).toBe(false)
    expect((await api.list_sessions(1))[0]?.ended_at_ms).toBe(c.now() - 20 * 60_000)
    const today = await api.get_view({ kind: 'today' })
    expect(today.active_ms + today.passive_ms).toBeGreaterThan(0)
  })
})

describe('mock DESIGN §7 / DESIGN §7.3 shapes', () => {
  const sessionView = async (api: ReturnType<typeof createMockBackend>) =>
    api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })

  it('attributes support work to the project: one project row (key code:<project>) with a category breakdown', async () => {
    const api = createMockBackend({ now: clock().now })
    const v = await sessionView(api)
    expectConsistent(v)
    const p = v.rows.find((r) => r.kind === 'project')
    expect(p).toMatchObject({ key: 'code:bank-agent-lab', label: 'bank-agent-lab', category: 'code' })
    expect(p?.breakdown.map((b) => b.key)).toEqual(expect.arrayContaining(['code', 'ai', 'docs', 'web']))
    expect(p?.breakdown.reduce((a, b) => a + b.ms, 0)).toBe(p?.ms)
    expect(v.rows.every((r) => r.kind === 'context' ? r.breakdown.length === 0 : true)).toBe(true)
    expect(v.segments.some((s) => s.explain.some((l) => l.includes('research for bank-agent-lab')))).toBe(true)
  })

  it('attribute_projects off → plain context rows', async () => {
    const api = createMockBackend({ now: clock().now })
    await api.set_config({ ...(await api.get_config()), attribute_projects: false })
    const v = await sessionView(api)
    expect(v.rows.every((r) => r.kind === 'context')).toBe(true)
    expectConsistent(v)
  })

  it('a user activity groups its members (beats project attribution), breakdown by member name', async () => {
    const api = createMockBackend({ now: clock().now })
    const cfg = await api.get_config()
    await api.set_config({ ...cfg, activities: [{ name: 'coding', apps: ['com.microsoft.VSCode', 'com.apple.Terminal'], domains: [] }] })
    const v = await sessionView(api)
    expectConsistent(v)
    const a = v.rows.find((r) => r.key === 'act:coding')
    expect(a).toMatchObject({ kind: 'activity', label: 'coding' })
    expect(a?.breakdown.map((b) => b.label)).toEqual(expect.arrayContaining(['VS Code']))
    expect(a?.details[0]?.detail).toMatch(/^bank-agent-lab · /)
    expect(v.segments.some((s) => s.explain.some((l) => l.includes('counted as coding')))).toBe(true)
  })

  it('status.current.key is the row key; project carries the project / activity name', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now })
    for (let i = 0; i < 6; i++) {
      c.advance(31_000)
      const st = await api.get_status()
      const v = await sessionView(api)
      const r = v.rows.find((x) => x.key === st.current?.key)
      expect(r).toBeDefined()
      expect(st.current?.context_ms).toBe(r?.ms)
      expect(st.current?.project).toBe(r?.kind === 'context' ? null : r?.label)
    }
  })

  it('focus line fields: longest focus segment and row switches', async () => {
    const v = await sessionView(createMockBackend({ now: clock().now }))
    const longest = Math.max(...v.segments.filter((s) => s.kind === 'focus').map((s) => s.end_ms - s.start_ms))
    expect(v.longest_focus_ms).toBe(longest)
    expect(v.switches).toBeGreaterThan(0)
  })

  it('view.one_liner summarizes the range, in the chosen language', async () => {
    const api = createMockBackend({ now: clock().now })
    expect((await sessionView(api)).one_liner).toMatch(/^\d+:\d\d in use · bank-agent-lab \d+:\d\d · away \d+m · longest focus \d+m$/)
    await api.set_prefs({ ...(await api.get_prefs()), language: 'tr' })
    expect((await sessionView(api)).one_liner).toMatch(/kullanımda · bank-agent-lab .* · uzakta \d+dk · en uzun odak \d+dk$/)
  })

  it('segments carry their project (null when unattributed)', async () => {
    const v = await sessionView(createMockBackend({ now: clock().now }))
    expect(v.segments.some((s) => s.project === 'bank-agent-lab')).toBe(true)
    expect(v.segments.find((s) => s.kind === 'away')?.project).toBeNull()
  })

  it('status.auto follows prefs.auto_track while tracking', async () => {
    const api = createMockBackend({ now: clock().now })
    expect((await api.get_status()).auto).toBe(false)
    await api.set_prefs({ ...(await api.get_prefs()), auto_track: true })
    expect((await api.get_status()).auto).toBe(true)
  })

  it('explain lines come translated when language is tr', async () => {
    const api = createMockBackend({ now: clock().now })
    await api.set_prefs({ ...(await api.get_prefs()), language: 'tr' })
    const v = await sessionView(api)
    expect(v.segments.find((s) => s.kind === 'away')?.explain[0]).toMatch(/^uzakta/)
  })

  it('seen_sources: apps and sites with time, ms desc, ≤ 50 each', async () => {
    const s = await createMockBackend({ now: clock().now }).seen_sources()
    expect(s.apps[0]).toMatchObject({ bundle_id: 'com.microsoft.VSCode', name: 'VS Code' })
    expect(s.domains.map((d) => d.domain)).toEqual(expect.arrayContaining(['github.com', 'chatgpt.com']))
    for (const list of [s.apps, s.domains]) {
      expect(list.length).toBeLessThanOrEqual(50)
      expect(list.map((x) => x.ms)).toEqual([...list.map((x) => x.ms)].sort((a, b) => b - a))
    }
  })
})

describe('mock DESIGN §8', () => {
  it('categories cover in-use time by each segment\'s own kind; shares sum to 1', async () => {
    const api = createMockBackend({ now: clock().now })
    const v = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expect(v.categories.reduce((a, c) => a + c.ms, 0)).toBe(v.active_ms + v.passive_ms)
    expect(v.categories.reduce((a, c) => a + c.share, 0)).toBeCloseTo(1, 6)
    expect(v.categories[0]?.category).toBe('code')
    expect(v.categories.map((c) => c.category)).toContain('ai') // project research still counts as ai here
  })
  it('listening: background audio, sorted desc, never added to in-use', async () => {
    const api = createMockBackend({ now: clock().now })
    const v = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expect(v.listening[0]).toMatchObject({ label: 'YouTube', title: 'lofi beats to code to' })
    expect(v.listening.map((l) => l.ms)).toEqual([...v.listening.map((l) => l.ms)].sort((a, b) => b - a))
    expectConsistent(v)
  })
})

describe('mock DESIGN §8 final shapes', () => {
  it('config carries app_categories (bundle id → category)', async () => {
    const cfg = await createMockBackend({ now: clock().now }).get_config()
    expect(cfg.app_categories['com.spotify.client']).toBe('media')
    expect(cfg.app_categories['com.tinyspeck.slackmacgap']).toBe('comms')
  })
  it('a listening entry can have no title (e.g. Spotify without track info)', async () => {
    const api = createMockBackend({ now: clock().now })
    const v = await api.get_view({ kind: 'today' })
    expect(v.listening.some((l) => l.title === null)).toBe(true)
  })
})

describe('mock DESIGN §11.5 history', () => {
  it('sessions_overview: newest first, paged, in-use and top row label per session', async () => {
    const api = createMockBackend({ now: clock().now })
    const page1 = await api.sessions_overview(3, 0)
    const page2 = await api.sessions_overview(3, 3)
    expect(page1).toHaveLength(3)
    expect(page1[0]?.ended_at_ms).toBeNull() // the open session first
    expect(page1.map((s) => s.started_at_ms)).toEqual([...page1.map((s) => s.started_at_ms)].sort((a, b) => b - a))
    expect(page2[0]?.started_at_ms).toBeLessThan(page1[2]?.started_at_ms ?? 0)
    const s = page1[1]!
    const v = await api.get_view({ kind: 'session', id: s.id })
    expect(s.in_use_ms).toBe(v.active_ms + v.passive_ms)
    expect(s.top_label).toBe(v.rows.find((r) => !r.key.startsWith('pass:'))?.label ?? null)
  })
  it('delete_session removes a closed session; the open one cannot be deleted', async () => {
    const api = createMockBackend({ now: clock().now })
    const [open, closed] = await api.sessions_overview(2, 0)
    await api.delete_session(closed!.id)
    expect((await api.sessions_overview(50, 0)).some((s) => s.id === closed!.id)).toBe(false)
    await expect(api.delete_session(open!.id)).rejects.toThrow(`session ${open!.id} is still being recorded`)
    await expect(api.delete_session(9999)).rejects.toThrow('no session with id 9999')
  })
  it('range all: every session, consistent', async () => {
    const api = createMockBackend({ now: clock().now })
    const all = await api.get_view({ kind: 'all' })
    const week = await api.get_view({ kind: 'week' })
    expectConsistent(all)
    expect(all.range).toEqual({ kind: 'all' })
    expect(all.total_ms).toBeGreaterThanOrEqual(week.total_ms)
  })
})

describe('mock export_json_string (copy = json)', () => {
  it('returns the export document for the range as a json string', async () => {
    const c = clock()
    const api = createMockBackend({ now: c.now })
    const id = (await api.get_status()).session_id ?? -1
    const doc = JSON.parse(await api.export_json_string({ kind: 'session', id }))
    expect(doc).toMatchObject({ schema: 'timewent.export.v1', range: { kind: 'session', id } })
    expect(doc.generated_at_ms).toBe(c.now())
    expect(doc.summary.rows.length).toBeGreaterThan(0)
    expect(doc.segments.length).toBeGreaterThan(0)
    expect(doc.sessions.map((s: { id: number }) => s.id)).toEqual([id])
  })
})

describe('mock language: system follows the OS preference, whatever the host is', () => {
  it('system + a Turkish Mac → Turkish explain lines and one-liner', async () => {
    pinLanguages(['tr-TR', 'en-US'])
    const api = createMockBackend({ now: clock().now })
    const v = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expect(v.one_liner).toMatch(/kullanımda/)
  })
  it('system + an English Mac → English', async () => {
    pinLanguages(['en-GB'])
    const api = createMockBackend({ now: clock().now })
    const v = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expect(v.one_liner).toMatch(/ in use · /)
  })
})

describe('mock not_shown (DESIGN §8.5)', () => {
  it('lists pass: time by label — in use, never a row, rows + not shown = in use', async () => {
    const api = createMockBackend({ now: clock().now })
    const v = await api.get_view({ kind: 'session', id: (await api.get_status()).session_id ?? -1 })
    expect(v.not_shown).toEqual([{ label: 'timewent', ms: 6_000 }])
    const rows = v.rows.reduce((a, r) => a + r.ms, 0)
    expect(rows + v.not_shown.reduce((a, n) => a + n.ms, 0)).toBe(v.active_ms + v.passive_ms)
  })
  it('empty when timewent is counted as itself', async () => {
    const api = createMockBackend({ now: clock().now })
    await api.set_config({ ...(await api.get_config()), count_self: true })
    expect((await api.get_view({ kind: 'today' })).not_shown).toEqual([])
  })
})
