// Controller: state, polling, actions, keyboard. Rendering lives in render/*.
//
// Layout (§10.2, unified card rule): one fixed-width window holding ONE card. Collapsed it is
// just the pill; expanding grows the same card downward (divider, then the panel). In the panel, rows
// and tape are fixed; only recent activity scrolls (its own ~5-row box); the footer is fixed.
import { api } from './api'
import { lang, onLangChange, resolveLang, setLang, t, T } from './i18n'
import { DRAG, h } from './dom'
import { copyTarget, nextReceipt, resolveRange, type Receipt, type RangeChoice } from './receipt'
import { nextPast, type Past } from './history'
import { tbtn } from './render/button'
import { formatDuration } from './format'
import { fitHeight } from './layout'
import { keyAction, type Action } from './keys'
import { onPeek, pickSavePath, setWindowSize } from './platform'
import { renderActivity } from './render/activity'
import { createFooter } from './render/footer'
import { createPill } from './render/pill'
import { createTabs } from './render/tabs'
import { createSettings, type SettingsTab } from './render/settings'
import { renderTape } from './render/tape'
import { renderHow } from './render/how'
import { tapeClick } from './tape'
import { renderLine } from './render/line'
import { detailsLabel, loadFlag, saveFlag } from './prefs'
import { renderWhere } from './render/where'
import type { Prefs, Range, SessionMeta, SessionOverview, Status, View } from './types'
import {
  howLines,
  fromLayout,
  inUseMs,
  inUseTitle,
  isFreshState,
  justEnded,
  liveKey,
  pillModel,
  rangeIsLive,
  receiptParts,
  summarySentence,
  rangeKey,
  rangeTabs,
  recentActivity,
  tabRange,
  toLayout,
  whereSummary,
  type TabId,
} from './viewmodel'

const WIDTH = 340 // pill and panel share it; expanding never widens the window
const PILL_H = 56 // collapsed card, borders included
const ANIM_MS = 160 // card height ease; 0 under prefers-reduced-motion
const PANEL = { min: 120, max: 640 } // panel height follows content up to max
const TOP_ROWS = 5
const METER_CELLS = 10
const STATUS_MS = 1000
const VIEW_MS = 5000

type Mode = 'collapsed' | 'expanded'

/** Scroll `box` so `el` is at its top — never scrolls the document (scrollIntoView would). */
function scrollWithin(box: HTMLElement, el: HTMLElement) {
  box.scrollTop += el.getBoundingClientRect().top - box.getBoundingClientRect().top
}

export function startApp(root: HTMLElement, initial: { mode?: Mode; settings?: boolean; settingsTab?: SettingsTab } = {}) {
  const state = {
    mode: 'collapsed' as Mode,
    pane: 'main' as 'main' | 'settings',
    status: null as Status | null,
    sessions: [] as SessionMeta[],
    choice: null as RangeChoice, // the user's tab pick (wins until relaunch); null = default
    receipt: null as Receipt, // §16.1: shown after stop until esc / collapse / start
    allDetails: new Set<string>(), // rows showing every detail (past the cap of 5)
    past: null as Past, // §19: a past session opened from history
    view: null as View | null,
    viewSig: '',
    openRows: new Set<string>(),
    openSeg: null as number | null,
    showAll: false,
    ended: null as { id: number; in_use_ms: number } | null, // just-stopped session summary
    todayInUse: null as number | null, // idle pill line 2
    todayAt: 0,
    peeking: false,
    busy: false,
  }
  let viewSeq = 0
  let statusTimer: ReturnType<typeof setInterval> | undefined
  let viewTimer: ReturnType<typeof setInterval> | undefined

  // ── components ──────────────────────────────────────────────
  const pill = createPill({
    toggleExpand: () => void setMode(state.mode === 'expanded' ? 'collapsed' : 'expanded'),
    track: () => void toggleTracking(),
    allowAccessibility: () => void api.open_accessibility_settings().catch(fail),
  })
  const tabs = createTabs(chooseTab)
  const footer = createFooter({ copy: () => void copyJson(), export: () => void exportJson(), settings: () => void toggleSettings() })
  footer.rangeSlot.append(tabs.el)

  const total = h('span', { class: 'in-use' })
  const cursor = h('span', { class: 'cursor', attrs: { 'aria-hidden': 'true' } }, '▍')
  const notice = h('div', { class: 'notice' }, T('ended'))
  const fresh = h(
    'div',
    { class: 'fresh' },
    h('div', null, h('span', { class: 'prompt' }, '$ '), T('tagline')),
    h('div', { class: 'dim' }, T('press_space')),
  )
  // §17 one answer by default: rows + one dim sentence; everything else under `▸ details`.
  const whereEl = h('div')
  const sentenceEl = h('div', { class: 'sentence' })
  const detailsBtn = h('button', { class: 'disclosure', attrs: { type: 'button' } })
  // the tape lives in the disclosure row: `▸ details  ▮▮▮▮░░▮▮█`; its axis shows only when open
  const tapeEl = h('div', { class: 'tape' })
  const axisEl = h('div', { class: 'tape-axis' })
  const tapeRow = h('div', { class: 'tape-row' }, detailsBtn, tapeEl, h('span'), axisEl)
  // inside details: away + ♪ | recent activity (own scroll)
  const howEl = h('div', { class: 'how' })
  const howRule = h('hr', { class: 'group' })
  const actEl = h('div', { class: 'acts' })
  const detailsEl = h(
    'div',
    { class: 'details' },
    howEl,
    howRule,
    h(
      'div',
      { class: 'sec-head', attrs: DRAG },
      h('span', { class: 'dim', attrs: DRAG }, T('recent')),
      h('span', { class: 'grow', attrs: DRAG }),
      h('span', { class: 'faint' }, T('latest_first')),
    ),
    actEl,
  )
  const sections = h(
    'div',
    null,
    h('div', { class: 'sec-head', attrs: DRAG }, h('span', { attrs: DRAG }, T('where'), cursor), h('span', { class: 'grow', attrs: DRAG }), total),
    whereEl,
    sentenceEl,
    h('hr', { class: 'group' }),
    tapeRow,
    detailsEl,
  )
  let detailsOpen = loadFlag('details', false)
  const paintDetails = () => {
    detailsBtn.textContent = detailsLabel(detailsOpen)
    detailsBtn.setAttribute('aria-expanded', String(detailsOpen))
    detailsEl.hidden = !detailsOpen
    axisEl.hidden = !detailsOpen // collapsed: no axis — the span is the row tooltip
  }
  paintDetails()
  onLangChange(paintDetails)
  detailsBtn.addEventListener('click', () => {
    detailsOpen = !detailsOpen
    saveFlag('details', detailsOpen)
    paintDetails()
    fit() // window height follows (existing resize path)
  })
  // two deliberate lines: "session ended  [copy]" / "bank-agent-lab 1:14 · away 13m"
  const receiptLine = h('span', { class: 'receipt-line' }, T('receipt_title'))
  const receiptParts2 = h('div', { class: 'receipt-parts' })
  const receiptCopy = tbtn(t('copy'), { title: t('copy_title'), onclick: () => void copyJson() })
  onLangChange(() => {
    receiptCopy.textContent = t('copy')
    receiptCopy.title = t('copy_title')
  })
  const receiptEl = h('div', { class: 'receipt' }, receiptLine, receiptCopy, receiptParts2)
  // §19: viewing a past session from history — `07.10 · 00:36–00:57   [← history]`
  const pastLabel = h('span', { class: 'past-label' })
  const pastBack = tbtn(t('back_history'), { title: t('back_title'), onclick: () => backToHistory() })
  onLangChange(() => (pastBack.textContent = t('back_history')))
  const pastEl = h('div', { class: 'past-strip' }, pastLabel, h('span', { class: 'grow' }), pastBack)
  pastEl.hidden = true
  const main = h('div', { class: 'pane pane-main' }, pastEl, receiptEl, notice, fresh, sections)
  const settingsEl = h('div', { class: 'pane pane-settings' })
  const settings = createSettings(settingsEl, {
    back: () => closeSettings(),
    saveConfig: async (c) => {
      const saved = await api.set_config(c)
      state.viewSig = '' // views re-derive from raw samples with the new config
      return saved
    },
    setPrefs: async (p) => {
      const saved = await api.set_prefs(p)
      applyLanguage(saved.language)
      return saved
    },
    seenSources: () => api.seen_sources(),
    openAccessibility: () => void api.open_accessibility_settings().catch(fail),
    quit: () => void quit(),
    rendered: () => fit(),
    sessionsOverview: (limit, offset) => api.sessions_overview(limit, offset),
    deleteSession: async (id) => {
      await api.delete_session(id)
      await refreshSessions()
      state.viewSig = ''
    },
    openSession: (o) => openPastSession(o),
    downloadAll: () => void exportJson({ kind: 'all' }),
  })
  const body = h('div', { class: 'body' }, main, settingsEl)
  const panel = h('div', { class: 'panel' }, body, footer.el)
  const card = h('div', { class: 'card' }, pill.el, panel)
  root.append(card)
  notice.hidden = fresh.hidden = receiptEl.hidden = true

  whereEl.addEventListener('click', (e) => {
    const key = (e.target as Element).closest<HTMLElement>('[data-key]')?.dataset.key
    if (!key) return
    if (!state.openRows.delete(key)) state.openRows.add(key)
    else state.allDetails.delete(key)
    renderView()
  })
  const pickSeg = (e: Event, toggle: boolean) => {
    const idx = (e.target as Element).closest<HTMLElement>('[data-seg]')?.dataset.seg
    if (idx === undefined) return
    const n = Number(idx)
    state.openSeg = toggle && state.openSeg === n ? null : n
    renderView()
    const row = actEl.querySelector<HTMLElement>(`.act[data-seg="${n}"]`)
    if (!toggle && row) scrollWithin(actEl, row)
  }
  actEl.addEventListener('click', (e) => pickSeg(e, true))
  // a tape cell opens details (if closed) and selects that segment in recent activity
  tapeEl.addEventListener('click', (e) => {
    const idx = (e.target as Element).closest<HTMLElement>('[data-seg]')?.dataset.seg
    if (idx === undefined) return
    const next = tapeClick({ detailsOpen, openSeg: state.openSeg }, Number(idx))
    if (next.detailsOpen !== detailsOpen) {
      detailsOpen = next.detailsOpen
      saveFlag('details', detailsOpen)
      paintDetails()
    }
    pickSeg(e, false)
    fit()
  })

  // ── data ────────────────────────────────────────────────────
  function effectiveRange(): Range {
    return resolveRange(state.choice, state.status, state.sessions) // §16.4: today in auto mode
  }

  function chooseTab(id: TabId) {
    leavePast('pick-range') // a range tab wins over the past session
    state.choice = tabRange(id) ?? 'session' // session = follow whatever session is current
    state.receipt = null // an explicit pick replaces the receipt's range
    state.openSeg = null
    state.showAll = false
    state.viewSig = ''
    updateRange()
    void refreshView()
  }

  function updateRange() {
    tabs.update(rangeTabs(effectiveRange(), state.sessions.length > 0, !!state.past))
  }

  // §19 past session: the row in history opens its report; esc / [← history] goes back there.
  let pastPrevChoice: RangeChoice = null
  function openPastSession(o: SessionOverview) {
    if (!state.past) pastPrevChoice = state.choice
    state.past = nextPast(state.past, { type: 'open', session: o })
    state.receipt = null
    state.choice = { kind: 'session', id: o.id }
    state.openSeg = null
    state.showAll = false
    state.viewSig = ''
    pastLabel.textContent = state.past?.label ?? ''
    pastEl.hidden = false
    closeSettings() // back to the panel, showing that session
    updateRange()
  }
  function leavePast(event: 'back' | 'pick-range' | 'collapse') {
    if (!state.past) return
    state.past = nextPast(state.past, { type: event })
    pastEl.hidden = true
    if (event !== 'pick-range') state.choice = pastPrevChoice
    state.viewSig = ''
  }
  function backToHistory() {
    leavePast('back')
    void openSettings('history')
  }

  function paintPill() {
    const st = state.status
    if (!st) return
    const latest = state.sessions[0]
    const fresh =
      latest && state.ended?.id === latest.id && justEnded(st, state.sessions, { kind: 'session', id: latest.id }, Date.now())
    pill.update(
      pillModel(st, Date.now(), { ended: fresh ? state.ended : null, todayInUseMs: state.todayInUse }),
      st.tracking,
      state.mode === 'expanded',
    )
  }

  // Just stopped → the pill says how long was in use. Needs that session's view once.
  async function loadEnded() {
    const latest = state.sessions[0]
    if (!latest || state.status?.tracking || state.ended?.id === latest.id) return
    if (!justEnded(state.status, state.sessions, { kind: 'session', id: latest.id }, Date.now())) return
    try {
      const v = await api.get_view({ kind: 'session', id: latest.id })
      state.ended = { id: latest.id, in_use_ms: inUseMs(v) }
      paintPill()
    } catch (e) {
      fail(e)
    }
  }

  function applyStatus(st: Status) {
    const prev = state.status
    state.status = st
    paintPill()
    cursor.hidden = !st.tracking // green = live: the cursor only blinks while tracking
    // A session started or stopped elsewhere (tray) → refresh what we show.
    if (prev?.tracking && !st.tracking && prev.session_id !== null) showReceipt(prev.session_id)
    if (prev && !prev.tracking && st.tracking) clearReceipt()
    if (prev && (prev.session_id !== st.session_id || prev.tracking !== st.tracking)) {
      state.viewSig = ''
      void refreshSessions().then(refreshView)
    } else if (prev?.current?.key !== st.current?.key && state.view) {
      renderView() // the live (green) row moved
    }
  }

  async function refreshStatus() {
    try {
      applyStatus(await api.get_status())
      if (!state.status?.tracking && Date.now() - state.todayAt > 60_000) void loadToday()
    } catch (e) {
      fail(e)
    }
  }

  // Idle pill line 2: "today 3:12:00 in use". Refreshed at most once a minute while idle.
  async function loadToday() {
    state.todayAt = Date.now()
    try {
      state.todayInUse = inUseMs(await api.get_view({ kind: 'today' }))
      paintPill()
    } catch (e) {
      fail(e)
    }
  }

  async function refreshSessions() {
    try {
      state.sessions = await api.list_sessions(20)
      updateRange()
      void loadEnded()
    } catch (e) {
      fail(e)
    }
  }

  async function refreshView() {
    if (state.mode !== 'expanded') return
    const seq = ++viewSeq
    const r = effectiveRange()
    try {
      const v = await api.get_view(r)
      if (seq !== viewSeq) return // a newer request is in flight
      renderNotices(v, r)
      const sig = JSON.stringify(v)
      if (sig !== state.viewSig) {
        state.view = v
        state.viewSig = sig
        if (state.openSeg !== null && state.openSeg >= v.segments.length) state.openSeg = null
        updateRange()
        renderView()
      }
      fit()
    } catch (e) {
      fail(e)
    }
  }

  // §16.1 stop = receipt: expand on the session that just ended, one bright line on top.
  function showReceipt(sessionId: number) {
    const prevChoice = state.receipt ? state.receipt.prevChoice : state.choice
    state.receipt = nextReceipt(state.receipt, { type: 'stopped', sessionId, choice: prevChoice })
    state.choice = { kind: 'session', id: sessionId }
    state.viewSig = ''
    void setMode('expanded') // user origin → set_layout; the backend keeps peek / position logic
  }
  function clearReceipt() {
    if (!state.receipt) return
    state.choice = state.receipt.prevChoice
    state.receipt = nextReceipt(state.receipt, { type: 'collapsed' })
    state.viewSig = ''
  }

  // Cheap, and time-dependent (the "ended" line fades out), so run on every refresh.
  function renderNotices(v: View, r: Range) {
    const isFresh = isFreshState(state.status, v)
    const onReceipt = !!state.receipt && r.kind === 'session' && r.id === state.receipt.sessionId
    fresh.hidden = !isFresh
    sections.hidden = isFresh
    receiptEl.hidden = !onReceipt || !v.one_liner
    renderLine(receiptParts2, receiptParts(v))
    notice.hidden = isFresh || onReceipt || !justEnded(state.status, state.sessions, r, Date.now())
  }

  function renderView() {
    const v = state.view
    if (!v) return
    total.textContent = t('in_use', { t: formatDuration(inUseMs(v)) })
    total.title = inUseTitle(v)
    const scroll = body.scrollTop
    const actScroll = actEl.scrollTop
    const summary = whereSummary(v, { limit: TOP_ROWS, showAll: state.showAll, liveKey: liveKey(state.status, effectiveRange()) }, METER_CELLS)
    renderWhere(whereEl, summary, state.openRows, { toggleAll, toggleDetails }, state.showAll, state.allDetails)
    const how = howLines(v)
    renderHow(howEl, how)
    howRule.hidden = how.length === 0
    const sentence = summarySentence(v)
    renderLine(sentenceEl, sentence?.parts ?? [])
    sentenceEl.title = sentence?.title ?? ''
    sentenceEl.hidden = !sentence
    tapeRow.title = renderTape(tapeEl, axisEl, v.segments, rangeIsLive(state.status, effectiveRange()))
    renderActivity(actEl, recentActivity(v), state.openSeg)
    body.scrollTop = scroll
    actEl.scrollTop = actScroll
    fit()
  }

  function toggleDetails(key: string) {
    if (!state.allDetails.delete(key)) state.allDetails.add(key)
    renderView()
  }

  function toggleAll() {
    state.showAll = !state.showAll
    renderView()
  }

  // Card height = pill + panel content (clamped); the window follows. Width never changes.
  // Measured from the parts, so it works while the card is still animating or clipped.
  let fitted = PILL_H
  function fit() {
    if (state.mode !== 'expanded') return
    const outer = (el: Element) => {
      const cs = getComputedStyle(el)
      return (el as HTMLElement).offsetHeight + parseFloat(cs.marginTop) + parseFloat(cs.marginBottom)
    }
    const pane = state.pane === 'settings' ? settingsEl : main
    const bs = getComputedStyle(body)
    const chrome = [...panel.children].filter((c) => c !== body).reduce((a, c) => a + outer(c), 0)
    const content = pane.offsetHeight + parseFloat(bs.paddingTop) + parseFloat(bs.paddingBottom)
    const divider = parseFloat(getComputedStyle(panel).borderTopWidth)
    const height = PILL_H + fitHeight(chrome + content + divider, PANEL)
    if (height !== fitted) void resizeCard(height).then(updateFade)
    updateFade()
  }

  // Growing: resize the (transparent) window to the end height first, then ease the card down
  // into it. Shrinking: ease the card up first, then shrink the window. Either way the window is
  // never smaller than the card, so nothing is clipped and no strip flashes.
  let resizeSeq = 0
  async function resizeCard(height: number) {
    const seq = ++resizeSeq
    const grow = height > fitted
    fitted = height
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches
    try {
      if (grow) {
        await setWindowSize(root, WIDTH, height)
        card.style.setProperty('--card-h', `${height}px`)
      } else {
        card.style.setProperty('--card-h', `${height}px`)
        if (!reduced) await new Promise((r) => setTimeout(r, ANIM_MS + 20))
        if (seq === resizeSeq) await setWindowSize(root, WIDTH, height)
      }
    } catch (e) {
      fail(e)
    }
  }

  // Fade a scroller's bottom edge only while there is more to scroll to.
  function updateFade() {
    for (const el of [body, actEl]) el.classList.toggle('fade', el.scrollTop + el.clientHeight < el.scrollHeight - 2)
  }
  body.addEventListener('scroll', updateFade, { passive: true })
  actEl.addEventListener('scroll', updateFade, { passive: true })
  window.addEventListener('resize', updateFade)

  // ── actions ─────────────────────────────────────────────────
  async function toggleTracking() {
    if (state.busy) return
    state.busy = true
    try {
      const st = state.status?.tracking ? await api.stop_session() : await api.start_session()
      state.todayAt = 0 // today's total changed
      applyStatus(st)
      await refreshSessions()
      state.viewSig = ''
      await refreshView()
    } catch (e) {
      fail(e)
    } finally {
      state.busy = false
    }
  }

  /** `user` changes are reported to the backend (set_layout); `backend` ones came from it. */
  async function setMode(mode: Mode, origin: 'user' | 'backend' = 'user') {
    if (mode === state.mode) return
    if (origin === 'user') void api.set_layout(toLayout(mode)).catch(fail)
    state.mode = mode
    paintPill()
    if (mode === 'expanded') {
      root.dataset.mode = mode // panel laid out (still clipped by the 56px card)
      await refreshSessions()
      state.viewSig = ''
      await refreshView() // renders, then fit() grows the card once, to the right height
      fit()
    } else {
      clearReceipt() // esc steps down through collapse: the receipt goes first
      leavePast('collapse')
      await resizeCard(PILL_H) // ease up, then shrink the window
      if (state.mode === 'collapsed') {
        root.dataset.mode = mode
        state.pane = 'main'
        root.dataset.pane = 'main'
      }
    }
    schedule()
  }

  async function quit() {
    try {
      await api.quit_app()
    } catch (e) {
      fail(e)
    }
  }

  async function hide() {
    try {
      await api.hide_window()
    } catch (e) {
      fail(e)
    }
  }

  async function openSettings(tab?: SettingsTab) {
    await setMode('expanded')
    try {
      const [cfg, info, prefs] = await Promise.all([api.get_config(), api.get_info(), api.get_prefs()])
      state.pane = 'settings'
      root.dataset.pane = 'settings'
      settings.open({ cfg, prefs, info, accessibility: state.status?.permissions.accessibility ?? null }, tab)
      body.scrollTop = 0
      fit()
      schedule()
    } catch (e) {
      fail(e)
    }
  }

  function closeSettings() {
    state.pane = 'main'
    root.dataset.pane = 'main'
    ;(document.activeElement as HTMLElement | null)?.blur()
    fit()
    schedule()
    void refreshView()
  }

  function toggleSettings() {
    if (state.pane === 'settings' && state.mode === 'expanded') closeSettings()
    else void openSettings()
  }

  // Copy = JSON (PLAN): every [copy] puts the export of what's on screen on the clipboard.
  async function copyJson() {
    try {
      const range = copyTarget({ past: state.past, receipt: state.receipt, effective: effectiveRange() })
            // Native pasteboard (§23.1): WKWebView refuses web clipboard writes after an await.
      try {
        await api.copy_json(range)
        footer.flash(t('copied'), 2000)
      } catch {
        footer.flash(`✗ ${t('copy_failed')}`, 4000)
      }
    } catch (e) {
      fail(e)
    }
  }

  async function exportJson(range?: Range) {
    try {
      const r = range ?? effectiveRange() // [json] exports what's on screen; history: everything
      const d = new Date()
      const day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
      const path = await pickSavePath(`timewent-${rangeKey(r)}-${day}.json`)
      if (!path) return
      footer.flash(`→ ${await api.export_json(r, path)}`)
    } catch (e) {
      fail(e)
    }
  }

  function fail(e: unknown) {
    footer.flash(`✗ ${e instanceof Error ? e.message : String(e)}`, 6000)
  }

  // ── timers ──────────────────────────────────────────────────
  // Status always (the pill shows elapsed); the view only while it is visible.
  function schedule() {
    clearInterval(statusTimer)
    clearInterval(viewTimer)
    statusTimer = viewTimer = undefined
    if (document.hidden) return
    statusTimer = setInterval(() => void refreshStatus(), STATUS_MS)
    if (state.mode === 'expanded' && state.pane === 'main') {
      viewTimer = setInterval(() => void refreshView(), VIEW_MS)
    }
  }

  document.addEventListener('visibilitychange', () => {
    schedule()
    if (!document.hidden) void refreshStatus().then(refreshView)
  })
  window.addEventListener('pagehide', () => {
    clearInterval(statusTimer)
    clearInterval(viewTimer)
  })

  // ── keyboard ────────────────────────────────────────────────
  document.addEventListener('keydown', (e) => {
    const t = e.target as Element | null
    const target = t?.closest?.('input, select, textarea') ? 'field' : t?.closest?.('button, [role="button"]') ? 'button' : 'none'
    const action = keyAction(
      { key: e.key, meta: e.metaKey, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey, target },
      { mode: state.mode, pane: state.pane, peeking: state.peeking },
    )
    // §19: while viewing a past session, esc returns to history (before the usual step-down)
    if (action === 'collapse' && e.key === 'Escape' && state.past && state.pane === 'main') {
      e.preventDefault()
      backToHistory()
      return
    }
    if (!action) return
    e.preventDefault()
    run(action)
  })

  function run(a: Action) {
    switch (a) {
      case 'track': return void toggleTracking()
      case 'expand': return void setMode('expanded')
      case 'collapse': return void setMode('collapsed')
      case 'back': return void (settings.back() || closeSettings())
      case 'hide': return void hide()
      case 'export': return void exportJson()
      case 'settings': return toggleSettings()
      case 'quit': return void quit()
      case 'keys': return void openSettings('keys')
      case 'range-session': return chooseTab('session')
      case 'range-today': return chooseTab('today')
      case 'range-week': return chooseTab('week')
      case 'end-peek': return void api.end_peek().catch(fail)
    }
  }
  // Mouse clicks shouldn't leave buttons focused, or space would press them again.
  document.addEventListener('click', (e) => {
    if (e.detail > 0 && (e.target as Element).closest('button')) (document.activeElement as HTMLElement | null)?.blur()
  })

  // ── peek (§10.4) ───────────────────────────────────────────
  // The backend owns the window: it shows/hides it and remembers the layout. The UI follows:
  // expanded while peeking; on close, whatever layout the event says to return to.
  void onPeek(({ open, mode }) => {
    state.peeking = open
    void setMode(open ? 'expanded' : fromLayout(mode), 'backend')
  })

  // ── language (§13.3) ───────────────────────────────────────
  // Static chrome relabels itself (T nodes); dynamic parts are re-rendered here. The backend
  // also switches explain lines / the one-liner, so the view is fetched again.
  function applyLanguage(pref: Prefs['language']) {
    const next = resolveLang(pref, navigator.languages ?? [navigator.language])
    if (next !== lang()) setLang(next)
  }
  const paintTitle = () => (document.title = `timewent — ${t('tagline')}`) // app name never translated
  onLangChange(() => {
    paintTitle()
    paintPill()
    updateRange()
    state.viewSig = ''
    void refreshView()
    if (state.pane === 'settings') settings.render()
  })

  // ── boot ────────────────────────────────────────────────────
  root.dataset.mode = 'collapsed'
  root.dataset.pane = 'main'
  void (async () => {
    await setWindowSize(root, WIDTH, PILL_H).catch(fail)
    applyLanguage((await api.get_prefs().catch(() => null))?.language ?? 'system')
    await refreshStatus()
    await refreshSessions()
    if (initial.settings) await openSettings(initial.settingsTab)
    else if (initial.mode === 'expanded') await setMode('expanded')
    schedule()
  })()
}
