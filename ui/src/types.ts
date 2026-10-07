// IPC contract — mirrors docs/PLAN.md §6 verbatim. snake_case on purpose: these are the
// exact JSON shapes the Rust side serializes. Change the plan first, then this file.

export type Category = 'code' | 'ai' | 'docs' | 'web' | 'media' | 'social' | 'comms' | 'design' | 'notes' | 'app'
export type Presence = 'active' | 'passive' | 'away'

export type Status = {
  tracking: boolean
  session_id: number | null
  started_at_ms: number | null
  elapsed_ms: number
  current: {
    key: string // the row key after attribution (proj:… / act:… / context key) — lights the live row
    label: string
    project: string | null // §11.1: when set and ≠ label, the pill shows it on line 1
    detail: string | null
    category: Category
    presence: Presence
    context_ms: number // time on this context in the current session (same segments as the view)
    since_ms: number | null // start of the current away/passive stretch
  } | null
  permissions: { accessibility: boolean }
  auto: boolean // §11.2: this session was started by auto mode
}

export type Range = { kind: 'session'; id: number } | { kind: 'today' } | { kind: 'week' } | { kind: 'all' } // week = local monday 00:00 → now

export type DetailTime = { detail: string; ms: number }

export type RowKind = 'project' | 'activity' | 'context'
/** project rows: key = category, label = english category word (translated in the UI);
 *  activity rows: key = bundle id / domain, label = member name (data). */
export type BreakdownItem = { key: string; label: string; ms: number }

export type Row = {
  key: string
  label: string
  category: Category
  ms: number
  share: number
  details: DetailTime[]
  kind: RowKind
  breakdown: BreakdownItem[]
}

export type SegmentKind = 'focus' | 'glance' | 'away' | 'gap'

export type SegmentDto = {
  start_ms: number
  end_ms: number
  key: string
  label: string
  category: Category | null
  kind: SegmentKind
  active_ms: number
  passive_ms: number
  details: DetailTime[]
  interruptions: { label: string; ms: number }[]
  explain: string[]
  project: string | null // §11.1 attribution
}

export type View = {
  range: Range
  total_ms: number
  active_ms: number
  passive_ms: number
  away_ms: number
  rows: Row[]
  segments: SegmentDto[] // chronological
  longest_focus_ms: number // §11.3
  switches: number // §11.3
  one_liner: string // §11.4, already in the resolved language
  categories: { category: Category; ms: number; share: number }[] // §14.1, over in-use time, desc
  listening: { label: string; title: string | null; ms: number }[] // §14.2, background audio, never in-use
}

export type SessionMeta = { id: number; started_at_ms: number; ended_at_ms: number | null }

/** §18/§19 history list row (closed sessions are cached by the backend). */
export type SessionOverview = {
  id: number
  started_at_ms: number
  ended_at_ms: number | null // null = the open session
  in_use_ms: number
  top_label: string | null
}

export type Info = { data_path: string; version: string }

/** Window layout as the backend knows it (set_layout, peek event). */
export type Layout = 'pill' | 'expanded'
/** Backend `peek` event. On close, `mode` is the layout to return to. */
export type PeekEvent = { open: boolean; mode: Layout }

/** Window/app preferences — not analysis config, so separate from Config. */
export type Prefs = {
  always_on_top: boolean
  peek_shortcut: string // Tauri accelerator
  auto_track: boolean // §11.2
  launch_at_login: boolean // §22, default true
  auto_split_after_s: number // §11.2, ≥ 300
  language: 'system' | 'en' | 'tr'
}

// Config is the core `Config` serde shape verbatim (PLAN §3.2; checked against
// crates/timewent-core/src/config.rs). Numbers are u32 on the Rust side: whole, non-negative.
// The backend re-validates with `Config::validate` and rejects with a message string.
export type Config = {
  poll_interval_ms: number
  passive_after_s: number
  away_after_s: number
  gap_after_s: number
  transient_max_s: number
  glance_max_s: number
  labels: Record<string, { label: string; category: Category }>
  docs_domains: string[]
  passthrough_bundle_ids: string[] // never a context; absorbed into neighbours (§10.1)
  count_self: boolean // count time spent looking at timewent itself (default false)
  support_window_s: number // §11.1
  attribute_projects: boolean // §11.1
  activities: Activity[] // §13.1, list order = match priority
  app_categories: Record<string, Category> // §14.1, bundle id → category
}

/** §13.1: a name you give to a set of apps (bundle ids) and sites (domains). */
export type Activity = { name: string; apps: string[]; domains: string[] }

/** §13.1: what timewent saw in the last 30 days, ms desc (max 50 + 50). */
export type SeenSources = {
  apps: { bundle_id: string; name: string; ms: number }[]
  domains: { domain: string; label: string; ms: number }[]
}

/** The backend surface — one method per §6 command. Implemented by Tauri `invoke` and by mock.ts. */
export interface Backend {
  start_session(): Promise<Status>
  stop_session(): Promise<Status>
  get_status(): Promise<Status>
  get_view(range: Range): Promise<View>
  list_sessions(limit: number): Promise<SessionMeta[]>
  export_json(range: Range, path: string): Promise<string>
    export_json_string(range: Range): Promise<string> // same content as the file export, pretty-printed
  copy_json(range: Range): Promise<void> // the same document, written to the macOS pasteboard natively
  get_config(): Promise<Config>
  set_config(config: Config): Promise<Config>
  get_info(): Promise<Info>
  open_accessibility_settings(): Promise<void>
  hide_window(): Promise<void>
  quit_app(): Promise<void> // ends any open session cleanly, then exits
    end_peek(): Promise<void> // esc during a peek: one step down — the pill, still focused
  set_layout(layout: Layout): Promise<void> // user expanded / collapsed
  seen_sources(): Promise<SeenSources> // §13.1
  sessions_overview(limit: number, offset: number): Promise<SessionOverview[]> // §19, newest first
  delete_session(id: number): Promise<void> // §19, never the open session
  get_prefs(): Promise<Prefs>
  set_prefs(prefs: Prefs): Promise<Prefs>
}
