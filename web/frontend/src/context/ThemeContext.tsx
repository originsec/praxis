import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react';
import { AVAILABLE_THEMES, DEFAULT_THEME, type ThemeName } from '../themes';

//
// Import both theme stylesheets so they're bundled. The active theme is
// controlled via the data-theme attribute on the html element.
//
import '../themes/origin_light/index.css';
import '../themes/praxis_dark/index.css';

const THEME_STORAGE_KEY = 'praxis-theme';

interface ThemeContextValue {
  theme: ThemeName;
  setTheme: (theme: ThemeName) => void;
  toggleTheme: () => void;
  isDark: boolean;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function getStoredTheme(): ThemeName {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored && AVAILABLE_THEMES.includes(stored as ThemeName)) {
      return stored as ThemeName;
    }
  } catch {
    // localStorage may not be available
  }
  return DEFAULT_THEME;
}

function applyTheme(theme: ThemeName): void {
  document.documentElement.dataset.theme = theme;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<ThemeName>(getStoredTheme);

  //
  // Apply theme on mount and when theme changes.
  //
  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  const setTheme = useCallback((newTheme: ThemeName) => {
    setThemeState(newTheme);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, newTheme);
    } catch {
      // localStorage may not be available
    }
  }, []);

  const toggleTheme = useCallback(() => {
    const currentIndex = AVAILABLE_THEMES.indexOf(theme);
    const nextIndex = (currentIndex + 1) % AVAILABLE_THEMES.length;
    setTheme(AVAILABLE_THEMES[nextIndex]);
  }, [theme, setTheme]);

  const isDark = theme === 'praxis_dark';

  return (
    <ThemeContext.Provider value={{ theme, setTheme, toggleTheme, isDark }}>
      {children}
    </ThemeContext.Provider>
  );
}

export function useTheme() {
  const context = useContext(ThemeContext);
  if (!context) {
    throw new Error('useTheme must be used within ThemeProvider');
  }
  return context;
}
