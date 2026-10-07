// Fake backend for `npm run dev` in a plain browser (and for tests).
// Generates a deterministic, internally consistent timeline from a script and the clock, so
// the UI sees the same shapes as DESIGN §11.10: sessions tick, contexts change, rows sum to totals,
// support work rolls up into a project row, user activities group their members.
import type {
  Activity, Backend, BreakdownItem, Category, Config, DetailTime, Prefs, Range, Row, RowKind, SeenSources,
  SegmentDto, SessionMeta, Status, View,
} from './types'

type Lang = 'en' | 'tr'
type Ctx = {
  key: string
  label: string
  category: Category
  app?: { bundle: string; name: string } // the frontmost app (activities match on bundle id)
  domain?: { domain: string; label: string } // browser host
}
type Item = {
  ctx: Ctx | 'away' | 'gap'
  s: number // seconds
  glance?: boolean
  details?: [string, number][] // detail, weight
  passive_s?: number
  interrupt?: { label: string; ms: number }
  proj?: { name: string; via: 'code' | 'match' | 'support' } // DESIGN §7.1 attribution
  bg?: { label: string; title: string | null } // DESIGN §8.4 audio playing behind the frontmost app
}

const ctx = (key: string, label: string, category: Category, src: Partial<Pick<Ctx, 'app' | 'domain'>> = {}): Ctx => ({
  key, label, category, ...src,
})
const CHROME = { bundle: 'com.google.Chrome', name: 'Google Chrome' }
const LAB = ctx('code:bank-agent-lab', 'bank-agent-lab', 'code', { app: { bundle: 'com.microsoft.VSCode', name: 'VS Code' } })
const GPT = ctx('web:chatgpt.com', 'ChatGPT', 'ai', { app: CHROME, domain: { domain: 'chatgpt.com', label: 'ChatGPT' } })
const CLAUDE = ctx('web:claude.ai', 'Claude', 'ai', { app: CHROME, domain: { domain: 'claude.ai', label: 'Claude' } })
const GH = ctx('web:github.com', 'GitHub', 'web', { app: CHROME, domain: { domain: 'github.com', label: 'GitHub' } })
const DOCS = ctx('web:docs', 'docs', 'docs', { app: CHROME, domain: { domain: 'docs.rs', label: 'docs.rs' } })
const YT = ctx('web:youtube.com', 'YouTube', 'media', { app: CHROME, domain: { domain: 'youtube.com', label: 'YouTube' } })
const TERM = ctx('app:Terminal', 'Terminal', 'code', { app: { bundle: 'com.apple.Terminal', name: 'Terminal' } })
// timewent itself: a passthrough app unless Config.count_self (then an ordinary context)
const SELF = ctx('pass:dev.timewent.app', 'timewent', 'app', { app: { bundle: 'dev.timewent.app', name: 'timewent' } })

const P = 'bank-agent-lab'
const code = { name: P, via: 'code' as const }
const LOFI = { label: 'YouTube', title: 'lofi beats to code to' }
const SPOTIFY = { label: 'Spotify', title: 'Weightless — Marconi Union' }
const SPOTIFY_UNKNOWN = { label: 'Spotify', title: null } // no track info available
const MAIN: Item[] = [
  { ctx: LAB, s: 1260, proj: code, bg: LOFI, details: [['risk_engine.py', 6], ['models.py', 4]], interrupt: { label: 'youtube.com', ms: 2000 } },
  { ctx: GPT, s: 540, proj: { name: P, via: 'support' }, details: [['Rolling VaR with pandas', 1]], passive_s: 100 },
  { ctx: LAB, s: 720, proj: code, bg: LOFI, details: [['risk_engine.py', 7], ['test_risk.py', 3]] },
  { ctx: DOCS, s: 300, proj: { name: P, via: 'support' }, details: [['docs.rs', 6], ['developer.mozilla.org', 4]] },
  { ctx: YT, s: 7, glance: true, details: [['lofi beats to code to - YouTube', 1]] },
  { ctx: LAB, s: 480, proj: code, details: [['risk_engine.py', 1]] },
  { ctx: 'away', s: 780 },
  { ctx: 'gap', s: 252 },
  { ctx: SELF, s: 6, glance: true }, // woke the mac, looked at timewent first: no host to absorb into
  { ctx: GH, s: 510, proj: { name: P, via: 'match' }, details: [['Pull request #42 · bank-agent-lab', 1]], interrupt: { label: 'Slack', ms: 2000 } },
  { ctx: LAB, s: 600, proj: code, bg: SPOTIFY, details: [['api.py', 5], ['risk_engine.py', 2]] },
  { ctx: CLAUDE, s: 420, details: [['Explain Basel III capital ratios', 1]], passive_s: 140 },
]

// Repeats after MAIN so a running session keeps changing context every 20–50s.
const LIVE: Item[] = [
  { ctx: LAB, s: 50, proj: code, bg: LOFI, details: [['risk_engine.py', 1]] },
  { ctx: GPT, s: 25, proj: { name: P, via: 'support' }, details: [['Rolling VaR with pandas', 1]] },
  { ctx: LAB, s: 40, proj: code, details: [['test_risk.py', 1]] },
  { ctx: DOCS, s: 20, details: [['docs.rs', 1]] },
  { ctx: YT, s: 6, glance: true },
  { ctx: TERM, s: 30, details: [['zsh — pytest -q', 1]] },
]

const PAST: Item[] = [
  { ctx: GH, s: 400, proj: { name: P, via: 'match' }, details: [['Issues · bank-agent-lab', 1]] },
  { ctx: LAB, s: 1500, proj: code, bg: SPOTIFY_UNKNOWN, details: [['models.py', 5], ['README.md', 1]], interrupt: { label: 'Mail', ms: 1000 } },
  { ctx: 'away', s: 300 },
  { ctx: GPT, s: 380, details: [['SQLAlchemy session scope', 1]], passive_s: 60 },
  { ctx: LAB, s: 900, proj: code, details: [['models.py', 1]] },
]

export const DEFAULT_CONFIG: Config = {
  poll_interval_ms: 1000,
  passive_after_s: 45,
  away_after_s: 180,
  gap_after_s: 5,
  transient_max_s: 3,
  glance_max_s: 10,
  labels: {
    'chatgpt.com': { label: 'ChatGPT', category: 'ai' },
    'chat.openai.com': { label: 'ChatGPT', category: 'ai' },
    'claude.ai': { label: 'Claude', category: 'ai' },
    'gemini.google.com': { label: 'Gemini', category: 'ai' },
    'github.com': { label: 'GitHub', category: 'web' },
    'youtube.com': { label: 'YouTube', category: 'web' },
    'google.com': { label: 'Google', category: 'web' },
    localhost: { label: 'localhost', category: 'code' },
  },
  docs_domains: [
    'developer.mozilla.org', 'docs.rs', 'doc.rust-lang.org', 'stackoverflow.com',
    'developer.apple.com', 'v2.tauri.app', 'docs.python.org',
  ],
  count_self: false,
  passthrough_bundle_ids: [
    'dev.timewent.app', 'com.apple.Spotlight', 'com.raycast.macos', 'com.runningwithcrayons.Alfred',
    'com.apple.dock', 'com.apple.controlcenter', 'com.apple.notificationcenterui', 'com.apple.screencaptureui',
    'com.apple.SecurityAgent', 'com.apple.UserNotificationCenter', 'com.apple.systemuiserver', 'com.apple.WindowManager',
  ],
  support_window_s: 600,
  attribute_projects: true,
  activities: [],
  app_categories: {
    'com.apple.Terminal': 'code', 'com.googlecode.iterm2': 'code', 'com.mitchellh.ghostty': 'code', 'dev.warp.Warp-Stable': 'code',
    'com.apple.dt.Xcode': 'code', 'com.postmanlabs.mac': 'code', 'com.tinyapp.TablePlus': 'code',
    'com.tinyspeck.slackmacgap': 'comms', 'com.apple.mail': 'comms', 'com.apple.MobileSMS': 'comms', 'net.whatsapp.WhatsApp': 'comms',
    'com.hnc.Discord': 'comms', 'us.zoom.xos': 'comms', 'com.microsoft.teams2': 'comms',
    'com.spotify.client': 'media', 'com.apple.Music': 'media',
    'com.figma.Desktop': 'design',
    'notion.id': 'notes', 'md.obsidian': 'notes', 'com.apple.Notes': 'notes',
  },
}

// The backend renders explain lines and the one-liner in the chosen language; so does the mock.
const pad2 = (n: number) => String(n).padStart(2, '0')
const hm = (ms: number) => `${Math.floor(ms / 3600_000)}:${pad2(Math.floor((ms % 3600_000) / 60_000))}`
const compact = (ms: number, l: Lang) => {
  const s = Math.floor(ms / 1000)
  const [h, m, u] = l === 'tr' ? ['sa', 'dk', 'sn'] : ['h', 'm', 's']
  return s >= 3600 ? `${Math.floor(s / 3600)}${h}${pad2(Math.floor((s % 3600) / 60))}${m}` : s >= 60 ? `${Math.floor(s / 60)}${m}` : `${s}${u}`
}
const X = {
  en: {
    away: (s: number, th: number) => `away: no input for ${s}s (threshold ${th}s)`,
    gap: (s: number) => `not tracked: no samples for ${s}s — sleep, app closed or between sessions`,
    absorbed: (label: string, s: string, th: number) => `absorbed ${label} (${s}s) — under transient threshold ${th}s`,
    passive: (d: string, th: number) => `passive ${d} — idle ≥ ${th}s, kept as reading`,
    support: (label: string, p: string) => `${label} counted as research for ${p} (between two coding blocks)`,
    match: (label: string, p: string) => `${label} matched project ${p}`,
    activity: (member: string, a: string) => `${member} counted as ${a} — your activity`,
    oneLiner: (inUse: string, top: string, away: string, focus: string) => `${inUse} in use · ${top} · away ${away} · longest focus ${focus}`,
  },
  tr: {
    away: (s: number, th: number) => `uzakta: ${s}sn boyunca giriş yok (eşik ${th}sn)`,
    gap: (s: number) => `kaydedilmedi: ${s}sn boyunca örnek yok — uyku, kapalı uygulama ya da oturum arası`,
    absorbed: (label: string, s: string, th: number) => `${label} (${s}sn) etkinliğe katıldı — ${th}sn eşiğinin altında`,
    passive: (d: string, th: number) => `pasif ${d} — ${th}sn+ boşta, okuma sayıldı`,
    support: (label: string, p: string) => `${label} → ${p} için araştırma (iki kodlama arasında)`,
    match: (label: string, p: string) => `${label} → ${p} projesiyle eşleşti`,
    activity: (member: string, a: string) => `${member} → ${a} (senin etkinliğin)`,
    oneLiner: (inUse: string, top: string, away: string, focus: string) => `${inUse} kullanımda · ${top} · uzakta ${away} · en uzun odak ${focus}`,
  },
}

/** Mock-internal: which row a segment belongs to (the backend does this in core). */
type Meta = { item: Item; ctx: Ctx }
const META = new WeakMap<SegmentDto, Meta>()

function gapSeg(start: number, end: number, l: Lang): SegmentDto {
  return {
    start_ms: start, end_ms: end, key: 'gap', label: 'gap', category: null, kind: 'gap',
    active_ms: 0, passive_ms: 0, details: [], interruptions: [], project: null,
    explain: [X[l].gap(Math.round((end - start) / 1000))],
  }
}

const hostMatches = (host: string, entry: string) => host === entry || host.endsWith(`.${entry}`)
function activityOf(c: Ctx, acts: readonly Activity[]): { name: string; member: { key: string; label: string } } | null {
  for (const a of acts) {
    if (c.domain && a.domains.some((d) => hostMatches(c.domain?.domain ?? '', d))) {
      return { name: a.name, member: { key: c.domain.domain, label: c.domain.label } }
    }
    if (c.app && a.apps.includes(c.app.bundle)) return { name: a.name, member: { key: c.app.bundle, label: c.app.name } }
  }
  return null
}

type RowOf = { key: string; label: string; kind: RowKind; member?: { key: string; label: string } }
function rowOf(seg: SegmentDto, cfg: Config): RowOf | null {
  const m = META.get(seg)
  if (!m || seg.key.startsWith('pass:')) return null // DESIGN §8.5: in use, never a row
  const act = activityOf(m.ctx, cfg.activities) // DESIGN §7.3: user intent beats inference
  if (act) return { key: `act:${act.name}`, label: act.name, kind: 'activity', member: act.member }
  if (cfg.attribute_projects && m.item.proj) return { key: `code:${m.item.proj.name}`, label: m.item.proj.name, kind: 'project' }
  return { key: seg.key, label: seg.label, kind: 'context' }
}

function makeSeg(it: Item, start: number, end: number, cfg: Config, l: Lang): SegmentDto {
  const dur = end - start
  if (it.ctx === 'gap') return gapSeg(start, end, l)
  if (it.ctx === 'away') {
    return {
      start_ms: start, end_ms: end, key: 'away', label: 'away', category: null, kind: 'away',
      active_ms: 0, passive_ms: 0, details: [], interruptions: [], project: null,
      explain: [X[l].away(Math.round(dur / 1000), cfg.away_after_s)],
    }
  }
  const c = it.ctx.key.startsWith('pass:') && cfg.count_self ? { ...it.ctx, key: 'app:timewent' } : it.ctx
  const passive = Math.min(dur, (it.passive_s ?? 0) * 1000)
  const weights = it.details ?? []
  const wsum = weights.reduce((a, [, w]) => a + w, 0)
  const details: DetailTime[] = weights
    .map(([detail, w]) => ({ detail, ms: Math.floor((dur * w) / wsum / 1000) * 1000 }))
    .filter((d) => d.ms > 0)
    .sort((a, b) => b.ms - a.ms)
  const explain: string[] = []
  const interruptions = it.interrupt && dur > it.interrupt.ms * 4 ? [it.interrupt] : []
  for (const i of interruptions) explain.push(X[l].absorbed(i.label, (i.ms / 1000).toFixed(1), cfg.transient_max_s))
  if (passive > 0) explain.push(X[l].passive(compact(passive, l), cfg.passive_after_s))
  const act = activityOf(c, cfg.activities)
  if (act) explain.push(X[l].activity(act.member.label, act.name))
  else if (cfg.attribute_projects && it.proj?.via === 'support') explain.push(X[l].support(c.domain?.domain ?? c.label, it.proj.name))
  else if (cfg.attribute_projects && it.proj?.via === 'match') explain.push(X[l].match(`${c.domain?.domain ?? c.label}/…/${it.proj.name}`, it.proj.name))
  const seg: SegmentDto = {
    start_ms: start, end_ms: end, key: c.key, label: c.label, category: c.category,
    kind: it.glance ? 'glance' : 'focus', active_ms: dur - passive, passive_ms: passive,
    details, interruptions, explain,
    // like core: members of a user activity are never attributed
    project: !act && cfg.attribute_projects ? (it.proj?.name ?? null) : null,
  }
  META.set(seg, { item: it, ctx: c })
  return seg
}

/** Plays `once` then cycles `loop` from `start` until `end`; the last item is cut at `end`. */
function timeline(start: number, end: number, once: Item[], loop: Item[], cfg: Config, l: Lang) {
  const segs: SegmentDto[] = []
  let t = start
  let last: Item | undefined
  for (let i = 0; t < end; i++) {
    const it = i < once.length ? once[i] : loop[(i - once.length) % loop.length]
    if (!it) break
    const e = Math.min(end, t + it.s * 1000)
    segs.push(makeSeg(it, t, e, cfg, l))
    last = it
    t = e
  }
  return { segs, last: last ?? loop[0] }
}

type Acc = { label: string; kind: RowKind; ms: number; details: Map<string, number>; parts: Map<string, BreakdownItem>; cats: Map<Category, number> }

function toView(range: Range, segments: SegmentDto[], cfg: Config, l: Lang = 'en'): View {
  let total = 0, active = 0, passive = 0, away = 0, longest = 0, switches = 0
  let prevRow: string | null = null
  const rows = new Map<string, Acc>()
  const hidden = new Map<string, number>() // DESIGN §8.5: pass: time, in use but no row
  const kinds = new Map<Category, number>() // DESIGN §8.3: each segment's own category
  const listening = new Map<string, { label: string; title: string | null; ms: number }>() // DESIGN §8.4
  for (const s of segments) {
    if (s.kind === 'gap') continue
    const dur = s.end_ms - s.start_ms
    total += dur
    if (s.kind === 'away') { away += dur; continue }
    active += s.active_ms
    passive += s.passive_ms
    kinds.set(s.category ?? 'app', (kinds.get(s.category ?? 'app') ?? 0) + dur)
    const bg = META.get(s)?.item.bg
    if (bg) {
      const k = `${bg.label}\u0000${bg.title ?? ''}`
      const l = listening.get(k) ?? { ...bg, ms: 0 }
      l.ms += dur // a parallel lane: never added to in-use
      listening.set(k, l)
    }
    const r = rowOf(s, cfg)
    if (!r) {
      if (s.key.startsWith('pass:')) hidden.set(s.label, (hidden.get(s.label) ?? 0) + dur)
      continue
    }
    if (s.kind === 'focus') longest = Math.max(longest, dur)
    if (prevRow !== null && prevRow !== r.key) switches++
    prevRow = r.key
    const acc = rows.get(r.key) ?? { label: r.label, kind: r.kind, ms: 0, details: new Map(), parts: new Map(), cats: new Map() }
    acc.ms += dur
    const cat = s.category ?? 'app'
    acc.cats.set(cat, (acc.cats.get(cat) ?? 0) + dur)
    // project: breakdown by category; activity: by member (label = data)
    const part = r.kind === 'project' ? { key: cat, label: cat } : r.kind === 'activity' && r.member ? r.member : null
    if (part) {
      const p = acc.parts.get(part.key) ?? { ...part, ms: 0 }
      p.ms += dur
      acc.parts.set(part.key, p)
    }
    const proj = META.get(s)?.item.proj?.name
    for (const d of s.details) {
      const detail = r.kind === 'activity' ? `${proj ?? s.label} · ${d.detail}` : d.detail
      acc.details.set(detail, (acc.details.get(detail) ?? 0) + d.ms)
    }
    rows.set(r.key, acc)
  }
  const inUse = active + passive // = the header's "in use"
  const out: Row[] = [...rows.entries()].map(([key, r]) => ({
    key,
    label: r.label,
    // project rows are code; others take their largest category
    category: r.kind === 'project' ? 'code' : [...r.cats].sort((a, b) => b[1] - a[1])[0]?.[0] ?? 'app',
    ms: r.ms,
    share: inUse > 0 ? r.ms / inUse : 0,
    details: [...r.details].map(([detail, ms]) => ({ detail, ms })).sort((a, b) => b.ms - a.ms),
    kind: r.kind,
    breakdown: [...r.parts.values()].sort((a, b) => b.ms - a.ms),
  }))
  const sorted = out.sort((a, b) => b.ms - a.ms || a.key.localeCompare(b.key))
  const top = sorted[0]
  return {
    range, total_ms: total, active_ms: active, passive_ms: passive, away_ms: away, segments, rows: sorted,
    longest_focus_ms: longest,
    switches,
    one_liner: X[l].oneLiner(hm(inUse), top ? `${top.label} ${hm(top.ms)}` : '—', compact(away, l), compact(longest, l)),
    categories: [...kinds]
      .map(([category, ms]) => ({ category, ms, share: inUse > 0 ? ms / inUse : 0 }))
      .sort((a, b) => b.ms - a.ms),
    listening: [...listening.values()].sort((a, b) => b.ms - a.ms),
    not_shown: [...hidden].map(([label, ms]) => ({ label, ms })).sort((a, b) => b.ms - a.ms),
  }
}

const TAKEN = new Set(['Cmd+Space', 'Ctrl+Space', 'Alt+Space']) // spotlight, input source, raycast-ish

type MockSession = SessionMeta & { once: Item[]; loop: Item[] }

/** Local time on the day `days` from `ms`'s day. */
function dayAt(ms: number, days: number, h: number, m: number): number {
  const d = new Date(ms)
  d.setDate(d.getDate() + days)
  d.setHours(h, m, 0, 0)
  return d.getTime()
}

export type MockScenario = 'running' | 'fresh' | 'stopped' | 'away' | 'idle'

export function createMockBackend(
  opts: { now?: () => number; accessibility?: boolean; scenario?: MockScenario; language?: Prefs['language']; auto?: boolean } = {},
): Backend {
  const now = opts.now ?? Date.now
  const t0 = now()
  const mainLen = MAIN.reduce((a, i) => a + i.s, 0) * 1000
  let config: Config = structuredClone(DEFAULT_CONFIG)
  let prefs: Prefs = {
    always_on_top: true,
    peek_shortcut: 'Alt+Shift+Space',
    auto_track: opts.auto ?? false,
    launch_at_login: true, // DESIGN §11.8: registered as a login item on first launch
    auto_split_after_s: 1800,
    language: opts.language ?? 'system',
  }
  const lang = (): Lang =>
    prefs.language === 'system' ? (globalThis.navigator?.languages?.[0]?.toLowerCase().startsWith('tr') ? 'tr' : 'en') : prefs.language
  const scenario = opts.scenario ?? 'running'
  // running: mid-session; stopped: that session ended 30s ago; fresh: never used;
  // away: running, but stepped away 4 minutes ago; idle: stopped 20 minutes ago.
  const endedAgo = scenario === 'stopped' ? 30_000 : scenario === 'idle' ? 20 * 60_000 : null
  const mainStart = t0 - mainLen - (endedAgo ?? (scenario === 'away' ? 240_000 : 30_000))
  const once = scenario === 'away' ? [...MAIN, { ctx: 'away' as const, s: 600 }] : MAIN
  const sessions: MockSession[] =
    scenario === 'fresh'
      ? []
      : [
          // A week of history: one or two sessions a day (realistic gaps overnight).
          ...[6, 5, 4, 3, 2].flatMap((d, i): MockSession[] => {
            const start = dayAt(t0, -d, 9 + (i % 3), 15 * i)
            return [
              { id: 1 + i * 2, started_at_ms: start, ended_at_ms: start + (5400 + 900 * i) * 1000, once: [], loop: PAST },
              ...(d % 2 === 0
                ? [{ id: 2 + i * 2, started_at_ms: start + 5 * 3600_000, ended_at_ms: start + 7 * 3600_000, once: [], loop: LIVE }]
                : []),
            ]
          }),
          { id: 11, started_at_ms: t0 - 23 * 3600_000, ended_at_ms: t0 - 23 * 3600_000 + 9000_000, once: [], loop: PAST },
          { id: 12, started_at_ms: t0 - 5 * 3600_000, ended_at_ms: t0 - 5 * 3600_000 + 3480_000, once: [], loop: PAST },
          { id: 13, started_at_ms: mainStart, ended_at_ms: endedAgo === null ? null : t0 - endedAgo, once, loop: LIVE },
        ]
  const open = () => sessions.find((s) => s.ended_at_ms === null)
  const segsOf = (s: MockSession) => timeline(s.started_at_ms, s.ended_at_ms ?? now(), s.once, s.loop, config, lang())

  function status(): Status {
    const s = open()
    const base = { permissions: { accessibility: opts.accessibility ?? true }, auto: false }
    if (!s) return { ...base, tracking: false, session_id: null, started_at_ms: null, elapsed_ms: 0, current: null }
    const { segs, last } = segsOf(s)
    // Like the backend: current = the last *real* context; away is a presence on top of it.
    const lastSeg = segs[segs.length - 1]
    const away = lastSeg?.kind === 'away'
    const host = [...segs].reverse().find((x) => (x.kind === 'focus' || x.kind === 'glance') && rowOf(x, config))
    const meta = host ? META.get(host) : undefined
    const row = host ? rowOf(host, config) : null
    const v = toView({ kind: 'session', id: s.id }, segs, config)
    const passive = !away && !!last?.passive_s
    return {
      ...base, auto: prefs.auto_track, tracking: true, session_id: s.id, started_at_ms: s.started_at_ms,
      elapsed_ms: now() - s.started_at_ms,
      current:
        host && meta && row
          ? {
              key: row.key, // the row key after attribution, so the live row lights up
              label: host.label,
              project: row.kind === 'context' ? null : row.label,
              category: meta.ctx.category,
              detail: (last === meta.item ? last.details?.[0]?.[0] : undefined) ?? host.details[0]?.detail ?? null,
              presence: away ? 'away' : passive ? 'passive' : 'active',
              context_ms: v.rows.find((r) => r.key === row.key)?.ms ?? 0,
              since_ms: away || passive ? (lastSeg?.start_ms ?? null) : null,
            }
          : null,
    }
  }

  function view(range: Range): View {
    if (range.kind === 'session') {
      const s = sessions.find((x) => x.id === range.id)
      return toView(range, s ? segsOf(s).segs : [], config, lang())
    }
    // today: since local midnight; week: since local monday 00:00 (DESIGN §11.10); all: everything.
    const dow = (new Date(now()).getDay() + 6) % 7 // monday = 0
    const from = range.kind === 'all' ? -Infinity : dayAt(now(), range.kind === 'week' ? -dow : 0, 0, 0)
    const segs: SegmentDto[] = []
    for (const s of [...sessions].sort((a, b) => a.started_at_ms - b.started_at_ms).filter((x) => x.started_at_ms >= from)) {
      const prev = segs[segs.length - 1]
      if (prev && s.started_at_ms > prev.end_ms) segs.push(gapSeg(prev.end_ms, s.started_at_ms, lang()))
      segs.push(...segsOf(s).segs)
    }
    return toView(range, segs, config, lang())
  }

  function seenSources(): SeenSources {
    const apps = new Map<string, { bundle_id: string; name: string; ms: number }>()
    const domains = new Map<string, { domain: string; label: string; ms: number }>()
    for (const s of sessions) {
      for (const seg of segsOf(s).segs) {
        const c = META.get(seg)?.ctx
        const dur = seg.end_ms - seg.start_ms
        if (c?.app && !c.app.bundle.startsWith('dev.timewent')) {
          const a = apps.get(c.app.bundle) ?? { bundle_id: c.app.bundle, name: c.app.name, ms: 0 }
          a.ms += dur
          apps.set(c.app.bundle, a)
        }
        if (c?.domain) {
          const d = domains.get(c.domain.domain) ?? { domain: c.domain.domain, label: c.domain.label, ms: 0 }
          d.ms += dur
          domains.set(c.domain.domain, d)
        }
      }
    }
    // a few long-tail apps so "show all" has something to show
    const tail = ['Slack', 'Mail', 'Notes', 'Figma', 'Finder', 'Preview', 'Calendar', 'Music', 'Zoom', 'Xcode', 'Postman', 'TablePlus', 'Obsidian', 'Linear', 'Spotify']
    tail.forEach((name, i) => apps.set(`app.mock.${name}`, { bundle_id: `app.mock.${name}`, name, ms: (tail.length - i) * 90_000 }))
    const desc = <T extends { ms: number }>(m: Map<string, T>) => [...m.values()].sort((a, b) => b.ms - a.ms).slice(0, 50)
    return { apps: desc(apps), domains: desc(domains) }
  }

  const later = <T>(v: T): Promise<T> => Promise.resolve(structuredClone(v))
  const backend: Backend = {
    start_session: () => {
      if (!open()) {
        const id = Math.max(0, ...sessions.map((s) => s.id)) + 1
        sessions.push({ id, started_at_ms: now(), ended_at_ms: null, once: [], loop: LIVE })
      }
      return later(status())
    },
    stop_session: () => {
      const s = open()
      if (s) s.ended_at_ms = now()
      return later(status())
    },
    get_status: () => later(status()),
    get_view: (range) => later(view(range)),
    list_sessions: (limit) =>
      later(
        [...sessions]
          .sort((a, b) => b.started_at_ms - a.started_at_ms)
          .slice(0, limit)
          .map(({ id, started_at_ms, ended_at_ms }) => ({ id, started_at_ms, ended_at_ms })),
      ),
    export_json: (_range, path) => later(path),
    // In a plain browser the web clipboard is all there is (dev only).
    copy_json: (range) =>
      backend.export_json_string(range).then((text: string) => navigator.clipboard?.writeText(text)),
    export_json_string: (range) => {
      // the same document the file export writes (schema timewent.export.v1), pretty-printed
      const v = view(range)
      const inRange = (s: MockSession) =>
        range.kind === 'session' ? s.id === range.id : v.segments.some((x) => x.start_ms >= s.started_at_ms && x.start_ms < (s.ended_at_ms ?? now()))
      const doc = {
        schema: 'timewent.export.v1',
        generated_at_ms: now(),
        range,
        config,
        sessions: sessions.filter(inRange).map(({ id, started_at_ms, ended_at_ms }) => ({ id, started_at_ms, ended_at_ms })),
        summary: {
          total_ms: v.total_ms, active_ms: v.active_ms, passive_ms: v.passive_ms, away_ms: v.away_ms,
          longest_focus_ms: v.longest_focus_ms, switches: v.switches, rows: v.rows, categories: v.categories, listening: v.listening,
        },
        segments: v.segments,
      }
      return later(`${JSON.stringify(doc, null, 2)}\n`) // like the backend: trailing newline
    },
    get_config: () => later(config),
    set_config: (c) => {
      config = structuredClone(c)
      return later(config)
    },
    get_info: () =>
      later({ data_path: '~/Library/Application Support/dev.timewent.app/timewent.db (mock)', version: '0.1.0-mock' }),
    open_accessibility_settings: () => Promise.resolve(),
    hide_window: () => Promise.resolve(),
    end_peek: () => Promise.resolve(), // no global key in a browser
    set_layout: () => Promise.resolve(),
    seen_sources: () => later(seenSources()),
    sessions_overview: (limit, offset) =>
      later(
        [...sessions]
          .sort((a, b) => b.started_at_ms - a.started_at_ms)
          .slice(offset, offset + limit)
          .map((s) => {
            const v = view({ kind: 'session', id: s.id })
            return {
              id: s.id,
              started_at_ms: s.started_at_ms,
              ended_at_ms: s.ended_at_ms,
              in_use_ms: v.active_ms + v.passive_ms,
              top_label: v.rows.find((r) => !r.key.startsWith('pass:'))?.label ?? null,
            }
          }),
      ),
    delete_session: (id) => {
      const i = sessions.findIndex((s) => s.id === id)
      // same messages as the backend
      if (i < 0) return Promise.reject(new Error(`no session with id ${id}`))
      if (sessions[i]?.ended_at_ms === null) return Promise.reject(new Error(`session ${id} is still being recorded`))
      sessions.splice(i, 1)
      return Promise.resolve()
    },
    quit_app: () => {
      console.info('[mock] quit_app — the real app ends the open session and exits')
      return Promise.resolve()
    },
    get_prefs: () => later(prefs),
    set_prefs: (p) => {
      // like the backend: registering a key another app owns fails and keeps the old one
      if (TAKEN.has(p.peek_shortcut)) return Promise.reject(new Error(`shortcut taken: ${p.peek_shortcut}`))
      prefs = { ...p }
      return later(prefs)
    },
  }
  return backend
}
