# timewent — design

> see where your time went.

timewent is a tiny, local-first macOS tracker. You press start (or let auto start do it), work,
and it tells you what you actually spent that time on, without you labelling anything. This
document describes how it works today: what it records, how it turns that into an answer, and
how the app behaves.

## 1. Principles

1. **Lightweight.** Small binary, low memory, about 0% CPU while idle. No framework where none is needed.
2. **A small tool, not a dashboard.** Black, monospace, terminal feel. One glance gives one answer;
   detail is one click away.
3. **Metadata only.** No screenshots, no keystrokes, no clipboard reads, no page or file contents.
   Input is measured as *seconds since the last event* per event type, so the app is
   structurally unable to log keys.
4. **Local only.** No network calls. Data never leaves the Mac.
5. **Raw ≠ interpreted.** Raw samples are stored and never changed. Everything you see is derived
   by a pure, deterministic function `f(samples, config)` and is recomputed whenever the config
   changes — including for past sessions.
6. **Explainable.** Every derived number can answer "why?": segments carry structured evidence that
   renders as plain sentences.
7. **Tested first.** The core crate has no I/O and is unit-tested, including golden tests on
   recorded sample streams.

## 2. Architecture

### 2.1 Crates

```
timewent-probe   (macOS, impure, thin)   one Sample per poll
      ▼
timewent-store   (SQLite)                raw samples, append-only
      ▼
timewent-core    (pure)                  context → presence → segments → attribution → summary → report
      ▼
src-tauri        (glue)                  tracking loop, IPC commands, tray, peek, login item
      ▼
ui/              (vanilla TypeScript)    the pill and the panel
```

| crate / dir | depends on | notes |
|---|---|---|
| `crates/timewent-core` | serde, url | No clock, filesystem, OS or randomness. Time always arrives as data. |
| `crates/timewent-store` | core, rusqlite (bundled) | Migrations by `PRAGMA user_version`. |
| `crates/timewent-probe` | core | `Probe` trait; `MacProbe` (real) and `FakeProbe` (scripted, for tests). Dev binary `timewent-probe-dump` prints samples as JSONL. |
| `src-tauri` | core, store, probe | Tauri 2 app, bundle id `dev.timewent.app`. |
| `ui` | `@tauri-apps/api` | Vite + vanilla TS, no UI framework. `mock.ts` runs the UI in a plain browser. |
| `fixtures` | — | Recorded/synthetic sample streams (`.jsonl`) and expected outputs. |

### 2.2 Data flow

A background thread samples every `poll_interval_ms` on absolute deadlines and appends to the
store. Views are computed on request from raw samples. The open session is kept in an
incremental segmenter that re-segments only the open tail; it is property-tested to stay
byte-identical to a full recompute. Closed sessions never change, so their summaries are cached
per config.

## 3. Samples

### 3.1 What a sample holds

| field | meaning |
|---|---|
| `ts_ms` | wall clock at poll time (unix ms) |
| `app_name`, `bundle_id` | the frontmost app |
| `window_title` | focused window title (needs Accessibility permission; empty otherwise) |
| `url` | the active tab's URL, browsers only, when obtainable |
| `idle` | seconds since the last key, mouse move, click and scroll — four numbers, never content |
| `locked` | screen locked, screensaver, or login window frontmost |
| `media_active` | something holds the display awake (video playback, a call) |
| `audio` | what is playing sound: `{ bundle_id, app, title, host }`, or nothing |

### 3.2 Where it comes from (macOS)

- **Frontmost app:** `NSWorkspace.frontmostApplication`.
- **Window title:** Accessibility API (`AXFocusedWindow` → `AXTitle`), 0.25 s messaging timeout.
- **URL:** AppleScript to the browser (Chromium family: active tab of the front window; Safari:
  front document; Firefox: not available). Asked only when (app, title) changes, or every 5 s for
  untitled windows. Timeout 750 ms; a timeout backs off 5 s, a denial 30 s.
- **Idle:** `CGEventSourceSecondsSinceLastEventType` for key down, mouse moved, left mouse down
  and scroll wheel. No event tap, no Input Monitoring permission.
- **Locked:** `CGSessionCopyCurrentDictionary`; `loginwindow` / `ScreenSaver.Engine` frontmost also count.
- **Media:** any process holding a `PreventUserIdleDisplaySleep` power assertion.
- **Audio:** a `PreventUserIdleSystemSleep` assertion owned by `coreaudiod` with `audio-out`; the
  player is its on-behalf-of process. One source per sample (not-frontmost first, then music
  players, browsers, others). Titles via AppleScript (Spotify/Music: `name — artist`; browsers: the
  first tab on a media host), only when the source changes or every 30 s.

Nothing is read while locked. A sample costs ~1.5 ms; a URL lookup (on change only) ~110–140 ms.

### 3.3 What is never collected

Keystrokes or their contents, screenshots, clipboard contents, page or file contents, and
anything from other users or the network. The only text recorded is what macOS already shows:
app names, window titles, URLs and now-playing titles.

## 4. Presence

Each sample is **active**, **reading** (passive) or **away**, from `idle = min(idle times)`:

| condition | presence |
|---|---|
| `locked` | away |
| `idle < passive_after_s` (45 s) | active |
| `idle < away_after_s` (180 s) | reading |
| otherwise | away |

- **Backdating:** within a maximal stretch of non-active samples (a gap breaks it), if any sample
  is away, every reading sample in that stretch becomes away. Reading then walking off is away
  from the start of the stretch; reading then scrolling stays reading.
- **Media and calls:** when `media_active` and not locked, idle never goes past reading.
  Media-held reading samples are exempt from backdating: watching stays watching even if you walk
  off afterwards.
- **In use** = active + reading. Away and untracked gaps never count.

## 5. Context derivation

Each sample gets a context `{ key, label, detail, category }`. First matching rule wins.

### 5.1 Code editors
VS Code, VS Code Insiders, Cursor, Windsurf. The title is parsed: leading dirty markers
(`●`, `•`, `* `) stripped, split on ` — ` (fallback ` - `), trailing app names dropped. Two or
more tokens → detail = file, label = project; one token → label only; none → the app name, and
such an editor (no parsed project) is an ordinary app context, never a project. Key `code:{label}`,
category `code`.

### 5.2 Browsers
Chrome, Arc, Brave, Safari, Edge, Firefox. With a URL, the host is lowercased and `www.` dropped:
- host in `labels` → that label and category (checked first, so explicit labels win);
- host in `docs_domains`, or `docs.*`, or `*.readthedocs.io` → key `web:docs`, label `docs`,
  detail = host, category `docs`;
- otherwise label = host, category `web`.

Detail is the page title. Without a URL: key `app:{app}`, category `web`.

### 5.3 Other apps
Key `app:{app}`, label = app name, detail = window title, category from `app_categories`
(bundle id → category), else `app`.

### 5.4 Categories
`code · ai · docs · web · media · social · comms · design · notes · app`. Rows say *what* (a
project, an activity, an app, a site); categories say *what kind*. Defaults:

| category | apps (`app_categories`) | sites (`labels`) |
|---|---|---|
| code | Terminal, iTerm2, Ghostty, Warp, Xcode, JetBrains IDEs, Postman, TablePlus | localhost |
| ai | — | chatgpt.com, chat.openai.com, claude.ai, gemini.google.com |
| media | Spotify, Music | youtube.com, music.youtube.com, netflix.com, twitch.tv, open.spotify.com, soundcloud.com |
| social | — | x.com, twitter.com, instagram.com, reddit.com, facebook.com, linkedin.com |
| comms | Slack, Mail, Messages, WhatsApp, Discord, Zoom, Teams | mail.google.com, outlook.live.com, app.slack.com, web.whatsapp.com, discord.com, meet.google.com, zoom.us |
| design | Figma | figma.com |
| notes | Notion, Obsidian, Notes | notion.so |
| web | — | github.com, google.com, docs.google.com |

Docs domains: developer.mozilla.org, docs.rs, doc.rust-lang.org, stackoverflow.com,
developer.apple.com, v2.tauri.app, docs.python.org. New built-in labels and app categories are
merged into a saved config on load; the user's own entries win.

### 5.5 Passthrough apps and timewent itself
Launchers and system UI that briefly take focus are never a context of their own:
Spotlight, Raycast, Alfred, Dock, Control Center, Notification Center, screenshot UI,
SecurityAgent, UserNotificationCenter, SystemUIServer, WindowManager and CaptiveNetworkAssistant
(`passthrough_bundle_ids`). timewent's own window follows `count_self` (default off): off, it is
passthrough; on, it is an ordinary app context.

## 6. Segmentation

### 6.1 Runs and gaps
Each sample's effective key is `away` when away, else its context key. Consecutive samples with
the same key form a run. A timestamp jump larger than `gap_after_s` ends the run and creates a
**gap** (sleep, crash, app not running) covering the missing time. A sample lasts until the next
sample in the same gap-free block (the last one lasts `poll_interval_ms`), so segments tile
tracked time exactly.

### 6.2 Absorption
A non-away run shorter than `transient_max_s` is a **transient** and never a segment of its own:
- between two runs with the same key, the three merge; the short run becomes an interruption;
- otherwise it attaches to the previous run, or the next one if there is no previous.

Its time counts toward the host. Hosts are never away and never across a gap; with no host on
either side, the run stays as its own glance, so time is never lost.

### 6.3 Kinds
Away runs → **away**. `transient_max_s ≤ duration < glance_max_s` → **glance**. Everything
else → **focus**.

### 6.4 Passthrough runs
A passthrough run shorter than `glance_max_s` is absorbed like a transient, whatever the
neighbour. A longer one becomes a `pass:{bundle_id}` glance that hosts nothing and is credited to
nobody: it counts toward in-use time but never becomes a row (see §8.5).

### 6.5 Thresholds

| setting | default | meaning |
|---|---|---|
| `poll_interval_ms` | 1000 | sample period (min 250) |
| `passive_after_s` | 45 | idle this long → reading |
| `away_after_s` | 180 | idle this long → away |
| `gap_after_s` | 5 | clock jump longer than this → not tracked |
| `transient_max_s` | 3 | runs shorter than this are absorbed |
| `glance_max_s` | 10 | runs shorter than this are glances; passthrough cap |
| `support_window_s` | 600 | research window for attribution (§7.2) |
| `attribute_projects` | true | turn project attribution off |
| `count_self` | false | count time spent looking at timewent |
| `auto_split_after_s` (pref) | 1800 | auto mode ends a session after this long away (min 300) |

Constraints: passive < away, transient < glance, and the gap threshold longer than one sample.
Invalid configs are rejected with a message naming the setting.

## 7. Attribution

### 7.1 Projects
Projects are the labels of code-editor contexts in the range (editors with no parsed project are
excluded). Names compare case-insensitively, with `-` equal to `_`. A segment matches a project
directly when:
- its URL is a repository URL (`github.com/{owner}/{repo}`, GitLab paths up to `/-/`) whose repo
  is a known project — authoritative; or
- its title contains the project name as a whole token (a terminal's directory, a chat title, a
  pull request title), covering at least 50% of the segment's time; or
- it is `localhost`, attributed to the project of the nearest code segment in the same block.

### 7.2 Research
An `ai` or `docs` segment between two blocks of the same project P counts for P when no away or
gap lies between them and the span is within `support_window_s`. Anchors are code segments and
direct matches; unattributed segments in between do not break it; plain `web` (including
YouTube) is never research.

Attributed segments roll up into one **project row** (key `code:{P}`, category `code`) with a
breakdown by category, e.g. `code 38m · ai 12m · docs 7m`.

### 7.3 Your activities
An activity is a name plus member apps (bundle ids) and sites (domains, subdomains included). A
sample whose app or host matches becomes `act:{name}`; apps match before domains, and the first
activity in list order wins. Your grouping beats inference: members are never attributed to a
project, never anchor one, and are never research. Activity rows have a breakdown by member
(`VS Code 38m · iTerm2 10m`), details from all members, and the category of the largest member.
The editor offers members from what timewent saw in the last 30 days (up to 50 apps and 50
sites, by time), so you never type a bundle id.

## 8. Summary

### 8.1 Rows and shares
Focus and glance time is grouped by row key — a project, an activity, or a context — sorted by
time, then key. Each row has its time, its **share of in-use time**, its breakdown, and its
details (files, pages), largest first. The panel shows the top 5; the rest are one click away.

### 8.2 Focus and switches
- **Longest focus:** the longest stretch of adjacent segments with the same row key; passthrough
  segments are transparent but add no time, away and gaps break it.
- **Switches:** boundaries between such stretches not separated by away or a gap.

### 8.3 Categories
Time per category over in-use time, using each segment's own category: research done for a
project still counts as `ai` here.

### 8.4 Listening (♪)
Audio playing from something that is *not* what is on screen: a background player, or a browser
whose visible tab is not the one playing. Locked time, calls and runs shorter than `glance_max_s`
(notification sounds) are ignored. Listening is a parallel lane: it is never added to any total.

### 8.5 Not shown
Uncredited passthrough time (§6.4) counts toward in use but has no row, so visible shares can sum
to less than 100%. The summary lists it separately as `not_shown`.

## 9. Explanations

Every segment can explain itself as short, terminal-comment-style lines:

```
# absorbed youtube.com (1.7s) — under transient threshold 3s
# away: no input for 212s (threshold 180s)
# passive 1m40s — idle ≥ 45s, kept as reading
# kept as passive 6m — media/meeting kept the display awake
# docs.rs counted as research for bank-agent-lab (between two bank-agent-lab blocks, 6m apart)
# github.com/acme/bank-agent-lab matched project bank-agent-lab
# iTerm2 counted as coding — your activity
# timewent 1m35s — passthrough, not credited to any activity
```

Text is English or Turkish; data (names, titles) is never translated. Compact durations are
`42s · 16m · 1h04m` (Turkish `42sn · 16dk · 1sa04dk`); clock times are the same in both. Every
view also has a one-line summary: `1:11 in use · bank-agent-lab 1:05 · away 13m · longest focus 47m`.

## 10. Report format

The export is **`timewent.report.v3`**: a compact narrative meant to be read by a person or any
LLM without a manual. Copy and Download produce the same bytes. Example: `docs/export-example.json`.

| field | content |
|---|---|
| `schema` | `"timewent.report.v3"` |
| `about` | one English sentence explaining the terms used in the file |
| `range` | `kind` (`session` · `today` · `week` · `all`), local `date`, `from`/`to` as local `HH:MM:SS` (dates for multi-day), IANA `timezone` |
| `totals` | `in_use_s, active_s, reading_s, away_s, not_tracked_s, longest_focus_s, switches` |
| `where` | top 10 rows: `{ name, kind: project·activity·app·site, category, seconds, share, breakdown?, top_details? (≤5) }` |
| `other_s` | seconds in rows beyond the top 10 |
| `listening` | `[{ title, source, seconds }]` |
| `sessions` | session/today only: `[{ from, to, in_use_s, blocks, short_visits }]` |
| `days` | week/all instead of `sessions`: `[{ date, in_use_s, top (≤3) }]` |
| `rules` | `passive_after_s, away_after_s, transient_max_s, glance_max_s` |
| `generated` | local ISO-8601 with offset |

- **Blocks** are chronological segments of 60 s or more (focus, and away of at least
  `away_after_s`); adjacent blocks with the same name merge. Each has `from, to, name, detail?,
  kind, seconds, why?`.
- **Short visits** sum everything shorter in a session: `{ count, seconds, top (≤5) }`. Gaps are
  implied by session boundaries.
- **`why`** appears only where it changes meaning: research attribution, project match, your
  activity, media kept as reading, and uncredited passthrough of 60 s or more. It is written in
  the app's language.
- Seconds are integers, shares have 2 decimals. No raw samples, no internal keys, no config dump.
  Local dates and times are computed per instant, so DST is handled. A day's report stays small
  (the `today` fixture is tested to stay under 8 KB).

## 11. App behaviour

### 11.1 The pill
One card, 340 px wide, always on top by default. Collapsed it is just the pill:

```
● bank-agent-lab                 38:14  [■]
  risk_engine.py · Session 1:38:27      [▾]
```

Line 1 is what you are on now (the project or activity when there is one) and its time this
session; line 2 the detail and the session clock (wall-clock time since start, away included).
The dot is green when active, amber when reading, red when away (`Away · since 14:48`). Not
tracking, line 1 reads `○ timewent` and line 2 `Today 3:12:00 in use`, `space to start`, or
`Ended · in use 1:21:15`. Without Accessibility, line 2 offers `Allow accessibility ↗`. Clicking
the text toggles the panel; dragging more than 4 px moves the window.

### 11.2 The panel
Expanding grows the same card downward (top edge fixed; the window never gets wider):

```
Where your time went                     In use 1:21:13
bank-agent-lab   ▮▮▮▮▮▮▮▮▮▯  91%  1:14:00
Claude           ▮▯▯▯▯▯▯▯▯▯   9%     7:00
Most: bank-agent-lab · Longest focus 21m
▸ Details                 ▮▮▮▮▮▮▮▮▮▮░░▁▮▮▮▮▮▮
Session · Today · Week                 [Copy] [Download] [⚙]
```

- Rows show an LED meter, the share of in-use time and the time; the live row's meter is green.
  Opening a row shows its breakdown and up to 5 details, then `+n more`.
- The in-use tooltip lists active, reading and away time, and every category with its share.
- The tape beside `▸ Details` is the range at a glance (cell height: active, reading, glance;
  hatched red: away). Clicking a cell opens details and selects that segment.
- `▸ Details` (state remembered) adds the tape's time axis, away and up to two ♪ lines, and
  recent activity (what, when, how long; latest first; its own scroll box). Opening a recent
  item shows its explanation.
- Ranges: Session (current or last), Today, Week (since Monday). Default is Today in auto mode,
  Session otherwise; a choice holds until relaunch.
- **Copy** puts the report JSON on the clipboard; **Download** saves it as a file.
- Nothing scrolls sideways; summary lines stay on one line and shorten names first.

### 11.3 Peek, hide and esc
- **Peek key** (default `⌥⇧Space`, changeable): shows the panel on the current Space, also over
  full-screen apps; pressing it again restores exactly the previous state (hidden stays hidden,
  pill stays pill). Left-click on the menu-bar icon does the same.
- **esc** steps down one level: settings → panel → pill → hidden. Hiding hands the keyboard back
  to the app you were in.
- `⌘W` hides; the app keeps running in the menu bar. The window's position never drifts; it is
  saved on quit and restored at launch.

### 11.4 Stop = receipt
Stopping expands the panel on the session that just ended, with a two-line receipt on top
(`Session ended  [Copy]` / `bank-agent-lab 1:14 · Away 13m`), cleared by the next collapse or start.

### 11.5 History
Settings → History lists past sessions, newest first, 20 at a time: date, time range, in-use time
and top row. Clicking a row opens that session's report, with a strip on top
(`06.10 · 02:21–04:51  [← History]`); esc returns to the list. `Delete` asks inline and removes the
session and its raw samples permanently; the open session cannot be deleted. `Download all`
exports range `all`. The data path and version sit at the bottom.

### 11.6 Settings
Tabs: **General** (language, keep on top, open at login, auto start, count timewent itself, peek
key, quit), **Tracking** (passive after, away after, auto split; the rest under `▸ Advanced`),
**Activities**, **History**, **Keys**. Labels are human, with units in words; the raw setting name
is only in the tooltip.

### 11.7 Auto start
Off by default. When on, a session starts on the first input after launch or after a split. A
session ends when away reaches `auto_split_after_s` (at the last input, or at the lock), or at
a sleep gap. Media never splits a session. While waiting, timewent checks idle every 5 s instead
of sampling. A manual stop pauses auto mode until a manual start, toggling it, or relaunch. The
pill shows `Auto` next to the time.

### 11.8 Open at login
On by default for the built app: a LaunchAgent starts timewent with `--login`, which shows the
pill without focus or activation at its last position. It is registered once; afterwards only
the toggle changes it, so a choice made in System Settings stands.

### 11.9 Languages
English and Turkish, or follow the system (Turkish if macOS prefers it). Labels start with a
capital; the brand `timewent`, comment lines, units, key-cap names and data stay as they are.

### 11.10 IPC commands

| command | purpose |
|---|---|
| `start_session`, `stop_session`, `get_status` | tracking state and what you are on now |
| `get_view(range)`, `list_sessions(limit)`, `sessions_overview(limit, offset)`, `delete_session(id)` | views and history |
| `export_json(range, path)`, `export_json_string(range)`, `copy_json(range)` | the report as a file, a string, or on the clipboard (native) |
| `get_config`, `set_config`, `get_prefs`, `set_prefs`, `seen_sources`, `get_info` | settings |
| `set_layout`, `resize_window`, `hide_window`, `end_peek`, `quit_app`, `open_accessibility_settings` | window and app |

`Range` is `{ kind: 'session', id } | { kind: 'today' } | { kind: 'week' } | { kind: 'all' }`.
Today and week are resolved in the system time zone by the app, never in core. The backend emits
a `peek` event `{ open, mode }`.

### 11.11 Keys

| key | action |
|---|---|
| `space` | start / stop |
| `↓` `↑` `↵` | expand / collapse |
| `esc` | step down (collapse, then hide) |
| `⌘W` | hide |
| `1` `2` `3` | Session · Today · Week |
| `⌘E` | download JSON |
| `⌘,` | settings |
| `⌘Q` | quit |
| `?` | show keys |
| `⌥⇧Space` | peek (global) |

## 12. Storage & privacy

Everything lives in `~/Library/Application Support/dev.timewent.app/`: `timewent.db` (SQLite),
`config.json` (analysis settings), `prefs.json` (app preferences), `window.json` (last position).

| table | content |
|---|---|
| `sessions` | `id, started_at_ms, ended_at_ms` (at most one open) |
| `contexts` | deduplicated `(bundle_id, app_name, window_title, url)` |
| `audio` | deduplicated now-playing sources |
| `samples` | `(session_id, ts_ms)` → context, four idle times (ms), `locked`, `media_active`, audio |

WAL mode, foreign keys on, migrations in place by `user_version`. An open session left by a crash
is closed at its last sample on launch. Deleting a session removes its samples, then contexts and
audio sources nothing uses any more, in one transaction. No data is sent anywhere; exports happen
only when you Copy or Download.

## 13. Platform notes

timewent runs on macOS (11+). Everything platform-specific is in `timewent-probe` (the `Probe`
trait: one `Sample` per call) plus a few window details in `src-tauri` (transparent always-on-top
window, Spaces / full-screen behaviour, login item, native pasteboard). Core, store and UI are
portable. A Windows or Linux port needs:

- a new `Probe`: foreground window and process, window title, browser URL where possible, idle
  time per input type (or one combined idle time), screen lock, and optionally media/audio state;
- the window, tray, global-shortcut and login-item glue for that platform;
- the same rule: metadata only, no input hooks that can read keys.
