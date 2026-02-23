/*
 * Theme system - supports runtime theme switching via ThemeContext.
 * Theme preference is persisted to localStorage.
 *
 * Available themes:
 * - origin: Brand-conformant light theme using Origin visual identity
 * - origin_light: Clean, minimal aesthetic with warm stone/bone tones
 * - praxis_dark: Retro terminal aesthetic with green phosphor glow
 */

export const AVAILABLE_THEMES = ['origin', 'praxis_dark', 'origin_light'] as const;
export type ThemeName = (typeof AVAILABLE_THEMES)[number];
export const DEFAULT_THEME: ThemeName = 'origin';
