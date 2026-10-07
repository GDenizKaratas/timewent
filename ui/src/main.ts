import './styles.css'
import { startApp } from './app'
import { isTauri } from './platform'
import type { SettingsTab } from './render/settings'

const root = document.getElementById('app')
if (root) {
  // Browser-only preview hooks (#expanded, #settings; mock states in api.ts).
  const hash = isTauri() ? '' : location.hash
  document.documentElement.classList.toggle('browser', !isTauri())
  startApp(root, {
    mode: hash.includes('expanded') ? 'expanded' : 'collapsed',
    settings: hash.includes('settings'),
    ...(/settings=(\w+)/.exec(hash)?.[1] ? { settingsTab: /settings=(\w+)/.exec(hash)?.[1] as SettingsTab } : {}),
  })
}
