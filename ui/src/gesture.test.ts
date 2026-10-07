import { describe, expect, it } from 'vitest'
import { gesture, IDLE, type Gesture } from './gesture'

const run = (events: Parameters<typeof gesture>[1][]) => {
  let g: Gesture = IDLE
  const actions: string[] = []
  for (const e of events) {
    const r = gesture(g, e)
    g = r.g
    if (r.action !== 'none') actions.push(r.action)
  }
  return actions
}

describe('pill click-vs-drag gesture', () => {
  it('down + up without moving is a click', () => {
    expect(run([{ type: 'down', x: 10, y: 10 }, { type: 'up', x: 10, y: 10 }])).toEqual(['click'])
  })
  it('jitter under 4px is still a click', () => {
    expect(run([{ type: 'down', x: 10, y: 10 }, { type: 'move', x: 12, y: 12 }, { type: 'up', x: 13, y: 11 }])).toEqual(['click'])
  })
  it('a move of 4px or more starts a drag exactly once, and no click follows', () => {
    expect(
      run([
        { type: 'down', x: 10, y: 10 },
        { type: 'move', x: 13, y: 10 },
        { type: 'move', x: 14, y: 10 },
        { type: 'move', x: 30, y: 10 },
        { type: 'up', x: 30, y: 10 },
      ]),
    ).toEqual(['drag'])
  })
  it('uses distance, not a single axis', () => {
    expect(run([{ type: 'down', x: 0, y: 0 }, { type: 'move', x: 3, y: 3 }])).toEqual(['drag']) // √18 ≈ 4.2
  })
  it('moves and ups without a down do nothing', () => {
    expect(run([{ type: 'move', x: 50, y: 50 }, { type: 'up', x: 50, y: 50 }])).toEqual([])
  })
  it('a new down resets a drag the OS swallowed the up of', () => {
    expect(
      run([{ type: 'down', x: 0, y: 0 }, { type: 'move', x: 9, y: 0 }, { type: 'down', x: 5, y: 5 }, { type: 'up', x: 5, y: 5 }]),
    ).toEqual(['drag', 'click'])
  })
  it('cancel abandons the gesture', () => {
    expect(run([{ type: 'down', x: 0, y: 0 }, { type: 'cancel', x: 0, y: 0 }, { type: 'up', x: 0, y: 0 }])).toEqual([])
  })
})
