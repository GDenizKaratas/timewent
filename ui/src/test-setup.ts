// Tests never depend on the host's locale: Node ≥ 21 exposes `navigator.languages` from the
// OS (e.g. tr-TR on a Turkish Mac), and the mock's `language: 'system'` follows it. Pin English;
// tests that exercise Turkish or the system choice say so explicitly.
import { beforeEach, vi } from 'vitest'

export const pinLanguages = (langs: readonly string[]) =>
  vi.stubGlobal('navigator', { ...(globalThis.navigator ?? {}), language: langs[0], languages: langs })

beforeEach(() => {
  vi.unstubAllGlobals()
  pinLanguages(['en-US'])
})
