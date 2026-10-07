import { describe, expect, it } from 'vitest'
import { fitHeight } from './layout'

describe('fitHeight', () => {
  it('uses the content height when it fits', () => {
    expect(fitHeight(312, { min: 160, max: 640 })).toBe(312)
  })
  it('clamps to max — the middle scrolls beyond that', () => {
    expect(fitHeight(1900, { min: 160, max: 640 })).toBe(640)
  })
  it('clamps to min so the panel never collapses to its chrome', () => {
    expect(fitHeight(90, { min: 160, max: 640 })).toBe(160)
  })
  it('rounds up to whole px so nothing is clipped by a fraction', () => {
    expect(fitHeight(300.2, { min: 160, max: 640 })).toBe(301)
  })
})
