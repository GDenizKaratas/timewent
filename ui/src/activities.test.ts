import { afterEach, describe, expect, it } from 'vitest'
import { checklist, memberSummary, removeActivity, toggleMember, upsertActivity, validateActivity } from './activities'
import { setLang } from './i18n'
import type { Activity, SeenSources } from './types'

const MIN = 60_000
const seen: SeenSources = {
  apps: [
    { bundle_id: 'com.microsoft.VSCode', name: 'VS Code', ms: 300 * MIN },
    { bundle_id: 'com.googlecode.iterm2', name: 'iTerm2', ms: 40 * MIN },
    ...Array.from({ length: 20 }, (_, i) => ({ bundle_id: `app.${i}`, name: `App ${i}`, ms: (20 - i) * MIN })),
  ],
  domains: [
    { domain: 'github.com', label: 'GitHub', ms: 90 * MIN },
    { domain: 'docs.rs', label: 'docs.rs', ms: 30 * MIN },
  ],
}
const coding: Activity = { name: 'coding', apps: ['com.microsoft.VSCode', 'com.googlecode.iterm2'], domains: [] }

describe('memberSummary (list line)', () => {
  afterEach(() => setLang('en'))
  it('names members from seen sources, apps then sites', () => {
    expect(memberSummary({ ...coding, domains: ['github.com'] }, seen)).toBe('VS Code · iTerm2 · GitHub')
  })
  it('falls back to the raw id for members not seen lately', () => {
    expect(memberSummary({ name: 'x', apps: ['com.gone.App'], domains: ['old.dev'] }, seen)).toBe('com.gone.App · old.dev')
  })
  it('says when there are none', () => {
    expect(memberSummary({ name: 'x', apps: [], domains: [] }, seen)).toBe('No members')
    setLang('tr')
    expect(memberSummary({ name: 'x', apps: [], domains: [] }, seen)).toBe('Üye yok')
  })
})

describe('checklist (edit view)', () => {
  it('apps first, then sites; each with its time; top 15 per group', () => {
    const c = checklist(seen, coding, false, 15)
    expect(c.apps).toHaveLength(15)
    expect(c.apps[0]).toEqual({ id: 'com.microsoft.VSCode', label: 'VS Code', time: '5h00m', checked: true })
    expect(c.sites.map((s) => s.label)).toEqual(['GitHub', 'docs.rs'])
    expect(c.hidden).toBe(7)
  })
  it('show all lists everything', () => {
    const c = checklist(seen, coding, true, 15)
    expect(c.apps).toHaveLength(22)
    expect(c.hidden).toBe(0)
  })
  it('a checked member never hides below the cut, and unseen members still appear (time —)', () => {
    const c = checklist(seen, { name: 'x', apps: ['app.19', 'com.gone.App'], domains: [] }, false, 15)
    expect(c.apps.map((a) => a.id)).toContain('app.19')
    expect(c.apps.find((a) => a.id === 'com.gone.App')).toEqual({ id: 'com.gone.App', label: 'com.gone.App', time: '—', checked: true })
  })
})

describe('editing', () => {
  it('toggleMember adds and removes, keeping order stable', () => {
    const a = toggleMember(coding, 'apps', 'app.1')
    expect(a.apps).toEqual(['com.microsoft.VSCode', 'com.googlecode.iterm2', 'app.1'])
    expect(toggleMember(a, 'apps', 'com.microsoft.VSCode').apps).toEqual(['com.googlecode.iterm2', 'app.1'])
    expect(toggleMember(coding, 'domains', 'github.com').domains).toEqual(['github.com'])
    expect(coding.apps).toHaveLength(2) // pure
  })
  it('upsert appends new ones, replaces in place when editing (list order = priority)', () => {
    const list = [coding, { name: 'reading', apps: [], domains: ['docs.rs'] }]
    expect(upsertActivity(list, null, { name: 'meetings', apps: ['us.zoom.xos'], domains: [] }).map((a) => a.name)).toEqual(['coding', 'reading', 'meetings'])
    expect(upsertActivity(list, 'coding', { ...coding, name: 'dev' }).map((a) => a.name)).toEqual(['dev', 'reading'])
  })
  it('remove by name', () => {
    expect(removeActivity([coding], 'coding')).toEqual([])
  })
})

describe('validateActivity', () => {
  afterEach(() => setLang('en'))
  it('needs a name, a member, and a unique name (case-insensitive, trimmed)', () => {
    expect(validateActivity({ name: '  ', apps: ['a'], domains: [] }, [], null)).toBe('Give it a name')
    expect(validateActivity({ name: 'x', apps: [], domains: [] }, [], null)).toBe('Pick at least one app or site')
    expect(validateActivity({ name: ' Coding ', apps: ['a'], domains: [] }, [coding], null)).toBe('That name is taken')
    expect(validateActivity({ name: 'coding', apps: ['a'], domains: [] }, [coding], 'coding')).toBeNull() // editing itself
    setLang('tr')
    expect(validateActivity({ name: '', apps: [], domains: [] }, [], null)).toBe('Bir ad ver')
  })
})
