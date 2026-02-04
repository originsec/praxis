/*
 * Theme system - supports static theme loading via CSS import.
 * To switch themes: change the import in main.tsx
 *
 * Available themes:
 * - origin_light: Clean, minimal aesthetic with warm stone/bone tones
 * - praxis_dark: Retro terminal aesthetic with green phosphor glow
 */

export const AVAILABLE_THEMES = ['origin_light', 'praxis_dark'] as const;
export type ThemeName = (typeof AVAILABLE_THEMES)[number];
export const DEFAULT_THEME: ThemeName = 'origin_light';
