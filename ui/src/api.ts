// Typed wrappers over the DESIGN §11.10 commands. Outside Tauri, the same calls go to mock.ts, which is
// loaded lazily so it never lands in the main bundle.
import { invoke } from '@tauri-apps/api/core'
import { isTauri } from './platform'
import type { Backend } from './types'

const tauri: Backend = {
  start_session: () => invoke('start_session'),
  stop_session: () => invoke('stop_session'),
  get_status: () => invoke('get_status'),
  get_view: (range) => invoke('get_view', { range }),
  list_sessions: (limit) => invoke('list_sessions', { limit }),
  export_json: (range, path) => invoke('export_json', { range, path }),
    export_json_string: (range) => invoke('export_json_string', { range }),
  copy_json: (range) => invoke('copy_json', { range }),
  get_config: () => invoke('get_config'),
  set_config: (config) => invoke('set_config', { config }),
  get_info: () => invoke('get_info'),
  open_accessibility_settings: () => invoke('open_accessibility_settings'),
  hide_window: () => invoke('hide_window'),
  quit_app: () => invoke('quit_app'),
  end_peek: () => invoke('end_peek'),
  set_layout: (layout) => invoke('set_layout', { layout }),
  seen_sources: () => invoke('seen_sources'),
  sessions_overview: (limit, offset) => invoke('sessions_overview', { limit, offset }),
  delete_session: (id) => invoke('delete_session', { id }),
  get_prefs: () => invoke('get_prefs'),
  set_prefs: (prefs) => invoke('set_prefs', { prefs }),
}

let mock: Promise<Backend> | undefined
function backend(): Promise<Backend> {
  if (isTauri()) return Promise.resolve(tauri)
  // Dev-only hash flags pick a mock state: #fresh, #stopped, #away, #idle, #noax, #auto, #tr / #en.
  const hash = location.hash
  const flags = hash.slice(1).split(',')
  mock ??= import('./mock').then((m) =>
    m.createMockBackend({
      accessibility: !flags.includes('noax'),
      auto: flags.includes('auto'),
      ...(flags.includes('tr') ? { language: 'tr' as const } : flags.includes('en') ? { language: 'en' as const } : {}),
      scenario: (['fresh', 'stopped', 'away', 'idle'] as const).find((s) => flags.includes(s)) ?? 'running',
    }),
  )
  return mock
}

export const api: Backend = {
  start_session: () => backend().then((b) => b.start_session()),
  stop_session: () => backend().then((b) => b.stop_session()),
  get_status: () => backend().then((b) => b.get_status()),
  get_view: (range) => backend().then((b) => b.get_view(range)),
  list_sessions: (limit) => backend().then((b) => b.list_sessions(limit)),
  export_json: (range, path) => backend().then((b) => b.export_json(range, path)),
    export_json_string: (range) => backend().then((b) => b.export_json_string(range)),
  copy_json: (range) => backend().then((b) => b.copy_json(range)),
  get_config: () => backend().then((b) => b.get_config()),
  set_config: (config) => backend().then((b) => b.set_config(config)),
  get_info: () => backend().then((b) => b.get_info()),
  open_accessibility_settings: () => backend().then((b) => b.open_accessibility_settings()),
  hide_window: () => backend().then((b) => b.hide_window()),
  quit_app: () => backend().then((b) => b.quit_app()),
  end_peek: () => backend().then((b) => b.end_peek()),
  set_layout: (layout) => backend().then((b) => b.set_layout(layout)),
  seen_sources: () => backend().then((b) => b.seen_sources()),
  sessions_overview: (limit, offset) => backend().then((b) => b.sessions_overview(limit, offset)),
  delete_session: (id) => backend().then((b) => b.delete_session(id)),
  get_prefs: () => backend().then((b) => b.get_prefs()),
  set_prefs: (prefs) => backend().then((b) => b.set_prefs(prefs)),
}
