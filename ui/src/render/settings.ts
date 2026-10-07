// In-panel settings (§13.2, §19): `[← back] settings`, then text tabs
// `general  tracking  activities  history  keys`, each opening with one dim `#` purpose line.
// Human labels with units in words; raw config keys only in tooltips.
// The activity editor is a sub-view with `[← cancel]` on the left (back always sits left).
import { checklist, memberSummary, removeActivity, toggleMember, upsertActivity, validateActivity } from '../activities'
import { h } from '../dom'
import { t, type Key } from '../i18n'
import { keysList } from '../keys'
import { confirmLayout, historyRow, nextConfirm, PAGE } from '../history'
import { advancedLabel, CONFIG_FIELDS, fromDisplay, withPref, thresholdRows, validateAutoSplit, validateThresholds, type ThresholdField } from '../settings-model'
import { autoHint } from '../viewmodel'
import { DEFAULT_PEEK, displayAccelerator, recordShortcut } from '../shortcut'
import type { Activity, Config, Info, Prefs, SeenSources, SessionOverview } from '../types'
import { tbtn } from './button'

export type SettingsTab = 'general' | 'tracking' | 'activities' | 'history' | 'keys'
const TABS: [SettingsTab, Key][] = [
  ['general', 'st_general'],
  ['tracking', 'st_tracking'],
  ['activities', 'st_activities'],
  ['history', 'st_history'],
  ['keys', 'st_keys'],
]
const CHECKLIST_TOP = 15

export type SettingsData = { cfg: Config; prefs: Prefs; accessibility: boolean | null; info: Info }
export type SettingsDeps = {
  back: () => void
  saveConfig: (c: Config) => Promise<Config>
  setPrefs: (p: Prefs) => Promise<Prefs>
  seenSources: () => Promise<SeenSources>
  openAccessibility: () => void
  quit: () => void
  rendered: () => void // layout changed (window height follows content)
  sessionsOverview: (limit: number, offset: number) => Promise<SessionOverview[]>
  deleteSession: (id: number) => Promise<void>
  openSession: (s: SessionOverview) => void // the row itself opens that session's report
  downloadAll: () => void
}

const PURPOSE: Record<SettingsTab, Key> = {
  general: 'pp_general',
  tracking: 'pp_tracking',
  activities: 'pp_activities',
  history: 'pp_history',
  keys: 'pp_keys',
}

type Edit = { prev: string | null; draft: Activity; showAll: boolean; error: string }

export function createSettings(el: HTMLElement, deps: SettingsDeps) {
  let data: SettingsData | null = null
  let tab: SettingsTab = 'general'
  let edit: Edit | null = null
  let seen: SeenSources | null = null
  let flash = { general: '', tracking: '' }
  let showAdvanced = false // tracking: `▸ advanced`, collapsed each time settings open
  let history: SessionOverview[] | null = null
  let historyMore = false
  let confirming: number | null = null
  let delError: { id: number; msg: string } | null = null // backend rejection, shown on that row

  function open(d: SettingsData, start: SettingsTab = 'general') {
    data = d
    tab = start
    edit = null
    flash = { general: '', tracking: '' }
    showAdvanced = false
    confirming = null
    history = null
    render()
  }

  /** esc: leave the editor first; returns true when it handled the key. */
  function back(): boolean {
    if (!edit) return false
    edit = null
    render()
    return true
  }

  function render() {
    if (!data) return
    // Fresh wrapper per render so listeners never stack on the persistent pane.
    const wrap = edit
      ? editView(data, edit)
      : h(
          'div',
          null,
          header(t('settings'), t('back'), deps.back),
          tabBar(),
          h('div', { class: 'comment purpose' }, t(PURPOSE[tab])),
          body(data),
        )
    el.replaceChildren(wrap)
    deps.rendered()
  }

  const header = (title: string, backText: string, onBack: () => void) =>
    h('div', { class: 'sub-head' }, tbtn(backText, { title: t('back_title'), onclick: onBack }), h('span', { class: 'sub-title' }, title))

  function tabBar() {
    return h(
      'div',
      { class: 'tabs set-tabs', attrs: { role: 'tablist' } }, // no `·` here: 5 tabs must fit 340px (§19)
      ...TABS.flatMap(([id, key]) => [
        choiceBtn(t(key), tab === id, () => {
          tab = id
          if (id === 'activities' && !seen) void loadSeen()
          render()
        }),
      ]),
    )
  }

  async function loadSeen() {
    try {
      seen = await deps.seenSources()
      render()
    } catch {
      seen = { apps: [], domains: [] }
    }
  }

  function body(d: SettingsData): HTMLElement {
    switch (tab) {
      case 'general':
        return general(d)
      case 'tracking':
        return tracking(d)
      case 'activities':
        if (!seen) void loadSeen()
        return activities(d)
      case 'history':
        if (!history) void loadHistory(0)
        return historyTab(d)
      case 'keys':
        return keysTab(d)
    }
  }

  // ── general ──────────────────────────────────────────────
  function general(d: SettingsData): HTMLElement {
    const msg = h('span', { class: 'msg' }, flash.general)
    const say = (text: string, err = false) => {
      flash.general = text
      msg.textContent = text
      msg.className = `msg${err ? ' err' : ''}`
    }
    const setPrefs = async (p: Prefs) => {
      d.prefs = await deps.setPrefs(p)
      return d.prefs
    }
    const langs: [Prefs['language'], Key][] = [['system', 'lang_system'], ['en', 'lang_en'], ['tr', 'lang_tr']]
    const language = h(
      'div',
      { class: 'set-line' },
      h('span', { class: 'k' }, t('language')),
      h(
        'span',
        { class: 'tabs' },
        ...langs.flatMap(([id, key], i) => [
          i ? h('span', { class: 'sep' }, '·') : '',
          choiceBtn(t(key), d.prefs.language === id, () => void setPrefs({ ...d.prefs, language: id }).catch((e) => say(`✗ ${String(e)}`, true))),
        ]),
      ),
    )
    const top = check(t('keep_on_top'), d.prefs.always_on_top, async (on) => (await setPrefs({ ...d.prefs, always_on_top: on })).always_on_top, (e) =>
      say(`✗ ${String(e)}`, true),
    )
    const autoHintEl = h('div', { class: 'hint' }, autoHint(d.prefs.auto_split_after_s))
    // §22: the backend registers / unregisters the login item as soon as the pref changes
    const login = check(
      t('launch_at_login'),
      d.prefs.launch_at_login,
      async (on) => (await setPrefs(withPref(d.prefs, 'launch_at_login', on))).launch_at_login,
      (e) => say(`✗ ${String(e)}`, true),
    )
    const auto = check(t('auto_start'), d.prefs.auto_track, async (on) => (await setPrefs({ ...d.prefs, auto_track: on })).auto_track, (e) =>
      say(`✗ ${String(e)}`, true),
    )
    const self = check(
      t('count_self'),
      d.cfg.count_self,
      async (on) => {
        d.cfg = await deps.saveConfig({ ...d.cfg, count_self: on })
        return d.cfg.count_self
      },
      (e) => say(`✗ ${String(e)}`, true),
    )

    // peek key: ⌥⇧Space [change] [reset]
    const peekCap = h('span', { class: 'peek-key' }, displayAccelerator(d.prefs.peek_shortcut))
    async function setPeek(accel: string) {
      try {
        await setPrefs({ ...d.prefs, peek_shortcut: accel })
        say(t('saved'))
      } catch {
        say(t('taken'), true) // backend keeps the old key
      }
      peekCap.textContent = displayAccelerator(d.prefs.peek_shortcut)
    }
    const record = tbtn(t('record'), { title: t('record_title') })
    let stop: (() => void) | null = null
    record.onclick = () => {
      if (stop) return stop()
      record.textContent = t('recording')
      say(t('record_hint'))
      const onKey = (e: KeyboardEvent) => {
        if (!record.isConnected) return stop?.() // settings closed mid-recording
        e.preventDefault()
        e.stopImmediatePropagation() // the app's own shortcuts must not fire while recording
        const r = recordShortcut(e)
        if (r.kind === 'wait') return
        if (r.kind === 'invalid') return say(r.reason, true)
        stop?.()
        if (r.kind === 'ok') void setPeek(r.accelerator)
        else say('')
      }
      window.addEventListener('keydown', onKey, true)
      stop = () => {
        window.removeEventListener('keydown', onKey, true)
        stop = null
        record.textContent = t('record')
      }
    }
    const reset = tbtn(t('reset'), {
      kind: 'quiet',
      title: t('reset_title', { k: displayAccelerator(DEFAULT_PEEK) }),
      onclick: () => void setPeek(DEFAULT_PEEK),
    })

    return h(
      'div',
      { class: 'set-body' },
      h('div', { class: 'comment' }, t('grp_look')),
      language,
      h('div', { class: 'check-line' }, top),
      h('div', { class: 'comment' }, t('grp_behaviour')),
      h('div', { class: 'check-line' }, login, h('div', { class: 'hint' }, t('launch_at_login_hint'))),
      h('div', { class: 'check-line' }, auto, autoHintEl),
      h('div', { class: 'check-line' }, self, h('div', { class: 'hint' }, t('count_self_hint'))),
      h('div', { class: 'comment' }, t('grp_peek')),
      h('div', { class: 'set-line peek-line' }, peekCap, h('span', { class: 'grow' }), record, reset),
      h('div', { class: 'peek-msg' }, msg),
      // permissions: only when something is missing
      d.accessibility === false
        ? h(
            'div',
            null,
            h('div', { class: 'comment' }, t('permissions')),
            h(
              'div',
              { class: 'set-line' },
              h('span', { class: 'k' }, t('accessibility')),
              h('span', { class: 'bad' }, t('not_granted')),
              h('span', { class: 'grow' }),
              tbtn(t('open_settings'), { title: t('open_settings_title'), onclick: deps.openAccessibility }),
            ),
          )
        : '',
      h('div', { class: 'quit' }, tbtn(t('quit'), { title: t('quit_title'), onclick: deps.quit })),
    )
  }

  // ── tracking ─────────────────────────────────────────────
  function tracking(d: SettingsData): HTMLElement {
    const msg = h('span', { class: 'msg' }, flash.tracking)
    const values: Record<ThresholdField, number> = {
      ...Object.fromEntries(CONFIG_FIELDS.map((f) => [f, d.cfg[f]])),
      auto_split_after_s: d.prefs.auto_split_after_s,
    } as Record<ThresholdField, number>
    const rows = thresholdRows(values, showAdvanced)
    const inputs = new Map<ThresholdField, HTMLInputElement>()
    const rowEl = (x: (typeof rows.main)[number]) => {
      const input = h('input', {
        attrs: { type: 'number', min: '1', step: '1', value: String(x.value), 'aria-label': x.label, spellcheck: 'false' },
      })
      inputs.set(x.field, input)
      // human label · value · unit in words; the raw key only in the (dim) tooltip
      return h('label', { class: 'set-row', title: x.title }, h('span', { class: 'k' }, x.label), input, h('span', { class: 'faint' }, x.unit))
    }
    let attribute = d.cfg.attribute_projects
    const attr = check(t('attribute_projects'), attribute, async (on) => ((attribute = on), on), () => {})
    const toggle = h('button', { class: 'disclosure', attrs: { type: 'button', 'aria-expanded': String(showAdvanced) } }, advancedLabel(showAdvanced))
    toggle.onclick = () => {
      showAdvanced = !showAdvanced
      render()
    }
    // inputs show minutes for long durations; convert back to the stored unit (seconds)
    const num = (f: ThresholdField) => {
      const shown = inputs.get(f)?.valueAsNumber
      return shown === undefined ? values[f] : fromDisplay(f, shown)
    }
    async function save() {
      const next: Config = { ...d.cfg, attribute_projects: attribute }
      for (const f of CONFIG_FIELDS) next[f] = num(f)
      const say = (text: string, bad: boolean) => {
        flash.tracking = text
        msg.textContent = text
        msg.className = `msg${bad ? ' err' : ''}`
      }
      const split = num('auto_split_after_s')
      const err = validateThresholds(next) ?? validateAutoSplit(split)
      if (err) return say(`✗ ${err}`, true)
      try {
        d.cfg = await deps.saveConfig(next)
        if (split !== d.prefs.auto_split_after_s) d.prefs = await deps.setPrefs({ ...d.prefs, auto_split_after_s: split })
        say(t('saved'), false)
      } catch (e) {
        say(`✗ ${String(e)}`, true)
      }
    }
    const box = h(
      'div',
      { class: 'set-body' },
      ...rows.main.map(rowEl),
      h('div', { class: 'hint first' }, t('hint_present')),
      h('div', { class: 'hint' }, t('hint_media')),
      h('div', { class: 'adv-head' }, toggle),
      rows.advanced
        ? h(
            'div',
            { class: 'advanced' },
            ...rows.advanced.map(rowEl),
            h('div', { class: 'check-line spaced' }, attr, h('div', { class: 'hint' }, t('attribute_hint'))),
          )
        : '',
      h('div', { class: 'set-actions' }, tbtn(t('save'), { kind: 'primary', title: t('save_title'), onclick: () => void save() }), msg),
    )
    box.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && e.target instanceof HTMLInputElement) void save()
    })
    return box
  }

  // ── history ──────────────────────────────────────────────
  async function loadHistory(offset: number) {
    try {
      const page = await deps.sessionsOverview(PAGE, offset)
      history = offset === 0 ? page : [...(history ?? []), ...page]
      historyMore = page.length === PAGE
      render()
    } catch {
      history = history ?? []
    }
  }

  function historyTab(d: SettingsData): HTMLElement {
    const list = history ?? []
    const rowEl = (o: SessionOverview) => {
      const r = historyRow(o)
      const row = h(
        'div',
        {
          class: `hist-row click${r.open ? ' now' : ''}${confirmLayout(o.id, confirming).highlight ? ' confirming' : ''}`,
          attrs: { role: 'button', tabindex: '0' },
          title: t('hist_open_title'),
        },
        h('span', { class: 'dim' }, r.date),
        h('span', null, r.range),
        h('span', { class: 'dim hist-inuse' }, r.inUse),
        delError?.id === o.id
          ? h('span', { class: 'hist-label hist-err', title: delError.msg }, `✗ ${delError.msg}`)
          : h('span', { class: `hist-label${r.labelMissing ? ' dim' : ''}` }, r.label),
        r.open
          ? h('span', { class: 'hist-now' }, `● ${t('now')}`)
          : tbtn(t('delete'), {
              kind: 'quiet',
              cls: 'hist-del',
              title: t('delete_title'),
              onclick: (e) => {
                e.stopPropagation() // not a click on the row
                confirming = nextConfirm(confirming, { type: 'ask', id: o.id })
                delError = null
                render()
              },
            }),
      )
      row.onclick = () => deps.openSession(o)
      row.onkeydown = (e) => {
        if (e.key === 'Enter') deps.openSession(o)
      }
      if (!confirmLayout(o.id, confirming).confirmBelow) return row
      // the row stays visible (highlighted); the confirm sits directly under it, indented
      const confirm = h(
        'div',
        { class: 'hist-confirm' },
        h('span', { class: 'bad' }, t('del_confirm')),
        tbtn(t('yes'), {
          onclick: async () => {
            try {
              await deps.deleteSession(o.id)
              confirming = nextConfirm(confirming, { type: 'done' })
              delError = null
              await loadHistory(0)
            } catch (e) {
              // e.g. "session 13 is still being recorded" — inline, dim red, never a crash
              confirming = nextConfirm(confirming, { type: 'cancel' })
              delError = { id: o.id, msg: e instanceof Error ? e.message : String(e) }
              render()
            }
          },
        }),
        tbtn(t('no'), { kind: 'quiet', onclick: () => ((confirming = nextConfirm(confirming, { type: 'cancel' })), render()) }),
      )
      return h('div', null, row, confirm)
    }
    return h(
      'div',
      { class: 'set-body' },
      h('div', { class: 'hist-head' }, tbtn(t('download_all'), { title: t('download_all_title'), onclick: deps.downloadAll })),
      history && list.length === 0 ? h('div', { class: 'comment' }, t('hist_empty')) : '',
      h('div', { class: 'hist-list' }, ...list.map(rowEl)),
      historyMore ? h('div', { class: 'more' }, tbtn(t('hist_more'), { kind: 'quiet', cls: 'text', onclick: () => void loadHistory(list.length) })) : '',
      h('div', { class: 'comment' }, t('data')),
      h('div', { class: 'path', title: d.info.data_path }, ...pathSegments(d.info.data_path)),
      h('div', { class: 'faint' }, `v${d.info.version}`),
    )
  }

  // ── activities ───────────────────────────────────────────
  function activities(d: SettingsData): HTMLElement {
    const list = d.cfg.activities
    const s = seen ?? { apps: [], domains: [] }
    const startEdit = (a: Activity | null) => {
      edit = { prev: a?.name ?? null, draft: a ? structuredClone(a) : { name: '', apps: [], domains: [] }, showAll: false, error: '' }
      if (!seen) void loadSeen()
      render()
    }
    return h(
      'div',
      { class: 'set-body' },
      list.length ? '' : h('div', { class: 'comment' }, t('act_empty')),
      ...list.map((a) =>
        h(
          'div',
          { class: 'act-item' },
          h('span', { class: 'act-name' }, a.name),
          h('span', { class: 'act-members', title: memberSummary(a, s) }, memberSummary(a, s)),
          tbtn(t('act_edit'), { onclick: () => startEdit(a) }),
          tbtn('×', {
            kind: 'quiet',
            cls: 'icon-sm',
            title: t('act_delete_title', { name: a.name }),
            onclick: async () => {
              d.cfg = await deps.saveConfig({ ...d.cfg, activities: removeActivity(d.cfg.activities, a.name) })
              render()
            },
          }),
        ),
      ),
      h('div', { class: 'set-actions' }, tbtn(t('act_new'), { kind: 'primary', onclick: () => startEdit(null) })),
    )
  }

  function editView(d: SettingsData, e: Edit): HTMLElement {
    const s = seen ?? { apps: [], domains: [] }
    const c = checklist(s, e.draft, e.showAll, CHECKLIST_TOP)
    const name = h('input', { class: 'name-input', attrs: { type: 'text', value: e.draft.name, placeholder: t('act_name_ph'), 'aria-label': t('act_name'), spellcheck: 'false' } })
    name.addEventListener('input', () => (e.draft.name = name.value))
    const err = h('span', { class: 'msg err' }, e.error)
    const item = (group: 'apps' | 'domains', i: { id: string; label: string; time: string; checked: boolean }) =>
      h(
        'div',
        { class: 'check-item' },
        check(i.label, i.checked, async () => {
          e.draft = toggleMember(e.draft, group, i.id)
          return e.draft[group].includes(i.id)
        }, () => {}),
        h('span', { class: 'dim' }, i.time),
      )
    async function save() {
      e.draft.name = name.value
      const problem = validateActivity(e.draft, d.cfg.activities, e.prev)
      if (problem) {
        e.error = problem
        err.textContent = problem
        return
      }
      try {
        d.cfg = await deps.saveConfig({ ...d.cfg, activities: upsertActivity(d.cfg.activities, e.prev, e.draft) })
        edit = null
        render()
      } catch (x) {
        err.textContent = `✗ ${String(x)}`
      }
    }
    // [← cancel]  new activity  [save] — save stays visible; only the checklist scrolls
    const head = header(e.prev === null ? t('act_title_new') : t('act_title_edit'), t('cancel'), () => back())
    head.append(h('span', { class: 'grow' }), tbtn(t('save'), { kind: 'primary', onclick: () => void save() }))
    const list = h(
      'div',
      { class: 'checklist' },
      seen && c.apps.length + c.sites.length === 0 ? h('div', { class: 'comment' }, t('act_none_seen')) : '',
      c.apps.length ? h('div', { class: 'comment split' }, h('span', null, t('act_apps')), h('span', null, t('act_seen'))) : '',
      ...c.apps.map((i) => item('apps', i)),
      c.sites.length ? h('div', { class: 'comment' }, t('act_sites')) : '',
      ...c.sites.map((i) => item('domains', i)),
      c.hidden > 0
        ? h('div', { class: 'more' }, tbtn(t('act_show_all', { n: c.hidden }), { kind: 'quiet', cls: 'text', onclick: () => ((e.showAll = true), render()) }))
        : '',
    )
    const fade = () => list.classList.toggle('fade', list.scrollTop + list.clientHeight < list.scrollHeight - 2)
    list.addEventListener('scroll', fade, { passive: true })
    requestAnimationFrame(fade)
    const view = h(
      'div',
      null,
      head,
      h('div', { class: 'set-body' }, h('label', { class: 'set-line' }, h('span', { class: 'k' }, t('act_name')), name), h('div', { class: 'act-err' }, err), list),
    )
    view.addEventListener('keydown', (k) => {
      if (k.key === 'Enter' && k.target === name) void save()
    })
    queueMicrotask(() => {
      if (e.prev === null && !e.draft.name) name.focus()
    })
    return view
  }

  // ── keys ─────────────────────────────────────────────────
  function keysTab(d: SettingsData): HTMLElement {
    return h(
      'div',
      { class: 'set-body' },
      h('div', { class: 'keys' },
        ...keysList(displayAccelerator(d.prefs.peek_shortcut)).flatMap(([caps, desc]) => [
          h('span', { class: 'caps' }, ...caps.split(' ').map((k) => h('kbd', null, k))),
          h('span', { class: 'dim' }, desc),
        ]),
      ),
    )
  }

  return {
    el,
    open,
    render,
    back,
    isEditing: () => edit !== null,
    showTab: (next: SettingsTab) => ((tab = next), (edit = null), render()),
    reloadHistory: () => void loadHistory(0),
  }
}

/** Text tab / choice: selected bright + underlined, others dim (same language as range tabs). */
function choiceBtn(text: string, on: boolean, pick: () => void): HTMLButtonElement {
  const b = h('button', { class: `tab${on ? ' on' : ''}`, attrs: { type: 'button', 'aria-selected': String(on) } }, text)
  b.onclick = pick
  return b
}

/** TUI checkbox `[x] label` / `[ ] label`; `commit` returns the value that stuck. */
function check(label: string, initial: boolean, commit: (on: boolean) => Promise<boolean>, onError: (e: unknown) => void) {
  let on = initial
  const b = tbtn('', { kind: 'quiet', cls: 'check' })
  b.setAttribute('role', 'switch')
  b.title = label
  const paint = () => {
    b.textContent = `[${on ? 'x' : ' '}] ${label}`
    b.setAttribute('aria-checked', String(on))
  }
  paint()
  b.onclick = async () => {
    try {
      on = await commit(!on)
      paint()
    } catch (e) {
      onError(e)
    }
  }
  return b
}

/** Path as unbreakable segments with a break opportunity after each "/" (text nodes only). */
function pathSegments(path: string): Node[] {
  // no regex lookbehind: Safari < 16.4 can't parse it, and the target is safari15
  const parts = path.split('/').map((p, i, all) => (i < all.length - 1 ? `${p}/` : p))
  return parts.filter(Boolean).flatMap((part) => [h('span', { class: 'seg' }, part), document.createElement('wbr')])
}
