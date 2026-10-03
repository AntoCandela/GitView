/** Shared pixel geometry for dividers; snap distances stay consistent regardless of pane size. */
export const dividerSize = 5;
const centerSnapDistance = 6;

/** Centers the divider stroke, leaving equal usable pane sizes on either side. */
export function snapResizeToCenter(value: number, available: number): number {
  const center = Math.max(0, available - dividerSize) / 2;
  return Math.abs(value - center) <= centerSnapDistance ? center : value;
}

export function constrainResize(value: number, min: number, max: number): number {
  return Math.max(Math.min(min, max), Math.min(value, max));
}
