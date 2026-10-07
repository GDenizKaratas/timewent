// TUI buttons: a 1px box around a monospace label; hover inverts, focus shows a ring.
import { h } from '../dom'

export type ButtonKind = 'primary' | 'neutral' | 'quiet'

export function tbtn(
  text: string,
  o: { title?: string; kind?: ButtonKind; onclick?: (e: MouseEvent) => void; cls?: string } = {},
): HTMLButtonElement {
  const b = h('button', { class: `tbtn ${o.kind ?? 'neutral'} ${o.cls ?? ''}`.trim(), attrs: { type: 'button' } }, text)
  if (o.title) {
    b.title = o.title
    b.setAttribute('aria-label', o.title)
  }
  if (o.onclick) b.onclick = o.onclick
  return b
}

/** Restyle an existing button in place (track button flips start ↔ stop). */
export function setButton(b: HTMLButtonElement, text: string, title: string, kind: ButtonKind) {
  b.textContent = text
  b.title = title
  b.setAttribute('aria-label', title)
  b.className = `tbtn ${kind}`
}
