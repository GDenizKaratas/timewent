import { describe, expect, it } from 'vitest'
import { detailsLabel, loadFlag, saveFlag } from './prefs'
import { setLang } from './i18n'

const mem = () => {
  const m = new Map<string, string>()
  return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }
}
const broken = {
  getItem: () => { throw new Error('SecurityError') },
  setItem: () => { throw new Error('QuotaExceeded') },
}

describe('details open state (§17, remembered per viewer)', () => {
  it('collapsed by default', () => {
    expect(loadFlag('details', false, mem())).toBe(false)
  })
  it('roundtrips', () => {
    const s = mem()
    saveFlag('details', true, s)
    expect(loadFlag('details', false, s)).toBe(true)
    saveFlag('details', false, s)
    expect(loadFlag('details', true, s)).toBe(false)
  })
  it('never throws when storage is blocked or missing; falls back to the default', () => {
    expect(loadFlag('details', false, broken)).toBe(false)
    expect(() => saveFlag('details', true, broken)).not.toThrow()
    expect(loadFlag('details', false, null)).toBe(false)
  })
})

describe('detailsLabel', () => {
  it('▸ when closed, ▾ when open; translated', () => {
    setLang('en')
    expect(detailsLabel(false)).toBe('▸ Details')
    expect(detailsLabel(true)).toBe('▾ Details')
    setLang('tr')
    expect(detailsLabel(false)).toBe('▸ Ayrıntılar')
    setLang('en')
  })
})
