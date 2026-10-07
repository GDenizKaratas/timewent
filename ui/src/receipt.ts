// DESIGN §11.4 stop = receipt, DESIGN §11.2 default range. Pure state rules; app.ts wires them.
import type { Range, SessionMeta, Status } from './types'

/** What the user picked: a range, 'session' (follow the latest session), or null (no pick yet). */
export type RangeChoice = Range | 'session' | null

/** After stop: the receipt line for the session that just ended, plus the choice to restore. */
export type Receipt = { sessionId: number; prevChoice: RangeChoice } | null
export type ReceiptEvent =
  | { type: 'stopped'; sessionId: number; choice: RangeChoice }
  | { type: 'started' }
  | { type: 'collapsed' } // collapse button, ↑/↵, or esc (esc steps down through collapse)
  | { type: 'refreshed' }

export function nextReceipt(r: Receipt, e: ReceiptEvent): Receipt {
  switch (e.type) {
    case 'stopped':
      return { sessionId: e.sessionId, prevChoice: e.choice }
    case 'started':
    case 'collapsed':
      return null
    default:
      return r
  }
}

/** No pick → today in auto mode, else the current/latest session. A pick wins until relaunch. */
export function resolveRange(choice: RangeChoice, st: Status | null, sessions: readonly SessionMeta[]): Range {
  if (choice && choice !== 'session') return choice
  if (choice === null && st?.auto) return { kind: 'today' }
  const id = st?.session_id ?? sessions[0]?.id
  return id === undefined || id === null ? { kind: 'today' } : { kind: 'session', id }
}

/** What `[copy]` copies (as json): a past session opened from history, else the session that
 *  just ended (receipt), else the range on screen. */
export function copyTarget(s: { past: { id: number; label?: string } | null; receipt: Receipt; effective: Range }): Range {
  if (s.past) return { kind: 'session', id: s.past.id }
  if (s.receipt) return { kind: 'session', id: s.receipt.sessionId }
  return s.effective
}
