// ASCII share bars: █ for the share, ░ for the rest.

/** Number of filled cells; any nonzero share gets at least one so small rows stay visible. */
export function barFill(share: number, width: number): number {
  if (width <= 0 || !Number.isFinite(share) || share <= 0) return 0
  return Math.max(1, Math.min(width, Math.round(Math.min(share, 1) * width)))
}

export function asciiBar(share: number, width: number): string {
  if (width <= 0) return ''
  const n = barFill(share, width)
  return '█'.repeat(n) + '░'.repeat(width - n)
}

/** A row's share of in-use time as a whole percent. A visible sliver never reads as 0%. */
export function formatShare(share: number): string {
  if (!Number.isFinite(share) || share <= 0) return '0%'
  const pct = Math.round(Math.min(share, 1) * 100)
  return pct === 0 ? '<1%' : `${pct}%`
}
