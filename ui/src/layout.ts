// Window sizing rules for the expanded panel.

/** Panel height = content height, clamped; beyond max the middle section scrolls. */
export function fitHeight(content: number, bounds: { min: number; max: number }): number {
  return Math.min(bounds.max, Math.max(bounds.min, Math.ceil(content)))
}
