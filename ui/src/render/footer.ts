// Footer: range tabs left; copy, json, settings right — plus a transient message slot.
// (Window controls — stop, collapse — live in the pill only.)
import { h } from '../dom'
import { onLangChange, t, type Key } from '../i18n'
import { tbtn } from './button'

export function createFooter(on: { copy: () => void; export: () => void; settings: () => void }) {
  const msg = h('span', { class: 'msg' })
  const rangeSlot = h('span', { class: 'range-slot' })
  const btn = (text: Key, title: Key, onclick: () => void, cls?: string) => {
    const b = tbtn(t(text), { title: t(title), onclick, ...(cls ? { cls } : {}) })
    onLangChange(() => {
      if (text !== title) b.textContent = t(text)
      b.title = t(title)
      b.setAttribute('aria-label', t(title))
    })
    return b
  }
  const settingsBtn = tbtn('⚙', { title: t('settings_title'), onclick: on.settings, cls: 'icon-sm' })
  onLangChange(() => (settingsBtn.title = t('settings_title')))
  const el = h(
    'footer',
    { class: 'foot' },
    rangeSlot,
    msg,
    h('span', { class: 'grow' }),
    btn('copy', 'copy_title', on.copy),
    btn('download', 'download_title', on.export), // always JSON (Copy = JSON, Download = file)
    settingsBtn,
  )
  let timer: ReturnType<typeof setTimeout> | undefined

  function flash(text: string, ms = 4000) {
    msg.textContent = text
    msg.title = text
    el.classList.add('flashing')
    clearTimeout(timer)
    timer = setTimeout(() => {
      msg.textContent = ''
      el.classList.remove('flashing')
    }, ms)
  }

  return { el, rangeSlot, flash }
}
