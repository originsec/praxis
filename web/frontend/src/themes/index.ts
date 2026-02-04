/*
 * Theme system - supports static theme loading via CSS import.
 * To switch themes: change the import in main.tsx
 *
 * Available themes:
 * - praxis_dark: Default retro terminal aesthetic
 */

export const AVAILABLE_THEMES = ['praxis_dark'] as const;
export type ThemeName = (typeof AVAILABLE_THEMES)[number];
export const DEFAULT_THEME: ThemeName = 'praxis_dark';
