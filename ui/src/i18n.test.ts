import { afterEach, describe, expect, it } from 'vitest'
import { DICTS, lang, resolveLang, setLang, t, type Key } from './i18n'

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()

describe('dictionaries', () => {
  it('en and tr have exactly the same keys', () => {
    expect(Object.keys(DICTS.tr).sort()).toEqual(Object.keys(DICTS.en).sort())
  })
  it('every string uses the same placeholders in both languages', () => {
    for (const k of Object.keys(DICTS.en) as (keyof typeof DICTS.en)[]) {
      expect(placeholders(DICTS.tr[k]), k).toEqual(placeholders(DICTS.en[k]))
    }
  })
  it('no empty strings', () => {
    for (const d of Object.values(DICTS)) for (const v of Object.values(d)) expect(v.trim()).not.toBe('')
  })
  it('the app name is always "timewent"', () => {
    for (const d of Object.values(DICTS)) {
      for (const v of Object.values(d)) if (/timewent/i.test(v)) expect(v).toMatch(/timewent/)
    }
    expect(DICTS.tr.quit).toMatch(/timewent/)
  })
})

/** Case rule (DESIGN §11.9): every UI label starts with a capital (tr locale: i → İ). Lowercase
 *  only for: the brand `timewent`, `#` comments, units, key-cap names, data. */
const LOWERCASE_OK = (k: string, v: string) =>
  /^(unit_|u_|cat_)/.test(k) || // units; category words are data (taxonomy values)
  v.startsWith('#') || // comments
  v.startsWith('timewent') || // brand
  /^(space|esc)\b/.test(v) // starts with a key-cap name

function caseLetter(s: string): string | null {
  const rest = s.replace(/^[\s✓✗←+…▸▾]+/u, '')
  if (/^[{⌘⌃⌥⇧]/u.test(rest)) return null // placeholder (data) or key cap first
  const m = /\p{L}/u.exec(rest)
  return m ? m[0] : null
}

describe('case rule, final', () => {
  for (const lang of ['en', 'tr'] as const) {
    it(`${lang}: every label starts with a capital, except brand, comments, units, key caps, data`, () => {
      const d = DICTS[lang]
      for (const k of Object.keys(d) as Key[]) {
        const c = caseLetter(d[k])
        if (c === null || LOWERCASE_OK(k, d[k])) continue
        expect(c === c.toLocaleUpperCase(lang) && c !== c.toLocaleLowerCase(lang), `${lang} ${k}: "${d[k]}"`).toBe(true)
      }
    })
  }
  it('timewent is always lowercase; json is always JSON', () => {
    for (const d of Object.values(DICTS)) {
      for (const v of Object.values(d)) {
        expect(v).not.toMatch(/Timewent|TIMEWENT/)
        expect(v).not.toMatch(/\bjson\b/i.test(v) ? /\bjson\b|\bJson\b/ : /$^/)
      }
    }
  })
  it('turkish capitals use the tr locale (i → İ)', () => {
    expect(DICTS.tr.auto_hint.startsWith('İlk')).toBe(true)
    expect(DICTS.tr.download).toBe('İndir')
  })
})

describe('settings purpose lines (DESIGN §11.6): one line at 340px', () => {
  it('every `#` purpose line is ≤ 42 chars in both languages', () => {
    for (const d of Object.values(DICTS)) {
      for (const k of ['pp_general', 'pp_tracking', 'pp_activities', 'pp_history', 'pp_keys'] as const) {
        expect([...d[k]].length, `${d[k]}`).toBeLessThanOrEqual(42)
      }
    }
  })
})

describe('t', () => {
  afterEach(() => setLang('en'))
  it('fills placeholders', () => {
    expect(t('in_use', { t: '1:11:31' })).toBe('In use 1:11:31')
  })
  it('follows the current language', () => {
    setLang('tr')
    expect(lang()).toBe('tr')
    expect(t('in_use', { t: '1:11:31' })).toBe('Kullanımda 1:11:31')
  })
})

describe('resolveLang', () => {
  it('explicit choices win', () => {
    expect(resolveLang('en', ['tr-TR'])).toBe('en')
    expect(resolveLang('tr', ['en-US'])).toBe('tr')
  })
  it('system: the first preferred language decides (tr → tr, else en)', () => {
    expect(resolveLang('system', ['tr-TR', 'en-US'])).toBe('tr')
    expect(resolveLang('system', ['en-GB', 'tr-TR'])).toBe('en')
    expect(resolveLang('system', ['de-DE'])).toBe('en')
    expect(resolveLang('system', [])).toBe('en')
  })
})
