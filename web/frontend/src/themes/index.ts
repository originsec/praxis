/*
 * Theme system - supports runtime theme switching via ThemeContext.
 * Theme preference is persisted to localStorage.
 *
 * Available themes:
 * - origin_light: Clean, minimal aesthetic with warm stone/bone tones
 * - praxis_dark: Retro terminal aesthetic with green phosphor glow
 */

export const AVAILABLE_THEMES = ['origin_light', 'praxis_dark'] as const;
export type ThemeName = (typeof AVAILABLE_THEMES)[number];
export const DEFAULT_THEME: ThemeName = 'praxis_dark';
