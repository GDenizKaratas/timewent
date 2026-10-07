import type { PeekEvent } from './types'

// Everything that differs between the Tauri webview and a plain browser (dev / mock).
// Tauri modules are imported lazily so the browser build never loads them.

export const isTauri = (): boolean => '__TAURI_INTERNALS__' in window

/** Resize the native window keeping its top-left corner (the backend does it in one frame,
 *  so peeks never drift); in a browser, size the shell so dev looks like the real window. */
export async function setWindowSize(el: HTMLElement, w: number, h: number): Promise<void> {
  if (!isTauri()) {
    el.style.setProperty('--win-w', `${w}px`)
    el.style.setProperty('--win-h', `${h}px`)
    return
  }
  const { invoke } = await import('@tauri-apps/api/core')
  await invoke('resize_window', { width: w, height: h })
}

/** Ask where to write an export. null = cancelled. */
export async function pickSavePath(fileName: string): Promise<string | null> {
  if (!isTauri()) return `~/Downloads/${fileName}`
  const { save } = await import('@tauri-apps/plugin-dialog')
  return save({ defaultPath: fileName, filters: [{ name: 'json', extensions: ['json'] }] })
}

/** Move the window with the pointer (pill drag past the click threshold). No-op in a browser. */
export async function startWindowDrag(): Promise<void> {
  if (!isTauri()) return
  const { getCurrentWindow } = await import('@tauri-apps/api/window')
  await getCurrentWindow().startDragging()
}

/** Backend `peek` event (DESIGN §11.3): the global key / tray click opened or closed a peek. */
export async function onPeek(cb: (e: PeekEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => {}
  const { listen } = await import('@tauri-apps/api/event')
  return listen<PeekEvent>('peek', (e) => cb(e.payload))
}

