// Per-viewer UI conveniences (DESIGN §11.2: details open/closed). Storage can be missing or throw
// (private mode, blocked site data), so every access is guarded and falls back to a default.
import { t } from './i18n'

type Store = Pick<Storage, 'getItem' | 'setItem'> | null

const defaultStore = (): Store => {
  try {
    return globalThis.localStorage ?? null
  } catch {
    return null
  }
}

const PREFIX = 'timewent.'

export function loadFlag(key: string, fallback: boolean, store: Store = defaultStore()): boolean {
  try {
    const v = store?.getItem(PREFIX + key)
    return v === '1' ? true : v === '0' ? false : fallback
  } catch {
    return fallback
  }
}

export function saveFlag(key: string, value: boolean, store: Store = defaultStore()): void {
  try {
    store?.setItem(PREFIX + key, value ? '1' : '0')
  } catch {
    // a convenience, not state — losing it is fine
  }
}

/** `▸ details` closed, `▾ details` open. */
export const detailsLabel = (open: boolean): string => `${open ? '▾' : '▸'} ${t('details')}`
