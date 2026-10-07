// Pill pointer gesture: a press is a click unless the pointer travels ≥ 4px, then it's a
// window drag (started once). Pure so the threshold rule is testable.

export type Gesture = { phase: 'idle' } | { phase: 'down'; x: number; y: number } | { phase: 'drag' }
export type PointerIn = { type: 'down' | 'move' | 'up' | 'cancel'; x: number; y: number }
export type GestureAction = 'none' | 'click' | 'drag'

export const IDLE: Gesture = { phase: 'idle' }
export const DRAG_THRESHOLD_PX = 4

export function gesture(g: Gesture, e: PointerIn): { g: Gesture; action: GestureAction } {
  switch (e.type) {
    case 'down':
      return { g: { phase: 'down', x: e.x, y: e.y }, action: 'none' }
    case 'move':
      if (g.phase === 'down' && Math.hypot(e.x - g.x, e.y - g.y) >= DRAG_THRESHOLD_PX) {
        return { g: { phase: 'drag' }, action: 'drag' }
      }
      return { g, action: 'none' }
    case 'up':
      return { g: IDLE, action: g.phase === 'down' ? 'click' : 'none' }
    case 'cancel':
      return { g: IDLE, action: 'none' }
  }
}
