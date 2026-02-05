export const SESSION_GROUP_COLORS = [
  //
  // Purple.
  //
  '#8B5CF6',
  //
  // Emerald.
  //
  '#10B981',
  //
  // Amber.
  //
  '#F59E0B',
  //
  // Red.
  //
  '#EF4444',
  //
  // Blue.
  //
  '#3B82F6',
  //
  // Pink.
  //
  '#EC4899',
  //
  // Teal.
  //
  '#14B8A6',
  //
  // Orange.
  //
  '#F97316',
] as const;

export type SessionGroupColor = (typeof SESSION_GROUP_COLORS)[number];

/**
 * Get the next available session group color that isn't already in use
 */
export function getNextSessionColor(usedColors: Set<string>): string {
  //
  // Find the first color not in use.
  //
  const available = SESSION_GROUP_COLORS.find(color => !usedColors.has(color));
  if (available) {
    return available;
  }
  //
  // If all colors are used, cycle back to the first one.
  //
  return SESSION_GROUP_COLORS[0];
}

/**
 * Extract all used colors from a list of chain elements
 */
export function getUsedColors(elements: { session_group?: { color: string } | null }[]): Set<string> {
  const colors = new Set<string>();
  for (const element of elements) {
    if (element.session_group?.color) {
      colors.add(element.session_group.color);
    }
  }
  return colors;
}

/**
 * Generate a lighter version of a color for backgrounds
 */
export function lightenColor(hex: string, percent: number = 90): string {
  //
  // Remove # if present.
  //
  const color = hex.replace('#', '');

  //
  // Parse RGB.
  //
  const r = parseInt(color.substring(0, 2), 16);
  const g = parseInt(color.substring(2, 4), 16);
  const b = parseInt(color.substring(4, 6), 16);

  //
  // Lighten.
  //
  const newR = Math.round(r + (255 - r) * (percent / 100));
  const newG = Math.round(g + (255 - g) * (percent / 100));
  const newB = Math.round(b + (255 - b) * (percent / 100));

  //
  // Convert back to hex.
  //
  return `#${newR.toString(16).padStart(2, '0')}${newG.toString(16).padStart(2, '0')}${newB.toString(16).padStart(2, '0')}`;
}
