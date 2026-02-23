const STORAGE_KEY = 'praxis_ui_mode';

export type UiMode = 'command_center' | 'legacy';

const DEFAULT_MODE: UiMode = 'command_center';

export function getUiMode(): UiMode {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === 'command_center' || stored === 'legacy') {
      return stored;
    }
  } catch {
    //
    // localStorage may not be available.
    //
  }
  return DEFAULT_MODE;
}

export function setUiMode(mode: UiMode): void {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    //
    // localStorage may not be available.
    //
  }
}
