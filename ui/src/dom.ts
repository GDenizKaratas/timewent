// Minimal DOM builder. Strings always become text nodes — window titles and URLs are
// untrusted, so nothing here ever touches innerHTML.

type Child = Node | string | null | undefined | false
type Props = {
  class?: string
  title?: string
  attrs?: Record<string, string>
  onclick?: (e: MouseEvent) => void
}

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Props | null = null,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag)
  if (props?.class) el.className = props.class
  if (props?.title) el.title = props.title
  if (props?.onclick) el.onclick = props.onclick
  for (const [k, v] of Object.entries(props?.attrs ?? {})) el.setAttribute(k, v)
  for (const c of children) if (c) el.append(c)
  return el
}

/** Replace all children (strings as text). */
export function fill(el: Element, ...children: Child[]): void {
  el.replaceChildren(...children.filter((c): c is Node | string => !!c))
}

export const DRAG = { 'data-tauri-drag-region': '' }
