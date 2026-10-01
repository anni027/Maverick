import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ResolvedTheme = 'light' | 'dark';

/** localStorage mirror of the active mode — read by the pre-paint inline
 *  script in index.html to avoid a theme flash before React boots. The
 *  backend `UiConfig.theme` (config.toml) remains the source of truth; App
 *  pushes it in via `setMode` after `get_ui_config` resolves. */
const STORAGE_KEY = 'maverick.theme';

interface ThemeState {
  mode: ThemeMode;
  resolved: ResolvedTheme;
  setMode: (mode: ThemeMode) => void;
  toggle: () => void;
}

const ThemeContext = createContext<ThemeState>({
  mode: 'dark',
  resolved: 'dark',
  setMode: () => {},
  toggle: () => {},
});

function readMode(): ThemeMode | null {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === 'light' || v === 'dark' || v === 'system' ? v : null;
  } catch {
    return null;
  }
}

function systemTheme(): ResolvedTheme {
  if (typeof window !== 'undefined' && window.matchMedia) {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
  }
  return 'dark';
}

function resolve(mode: ThemeMode): ResolvedTheme {
  return mode === 'system' ? systemTheme() : mode;
}

function applyTheme(resolved: ResolvedTheme): void {
  if (typeof document === 'undefined') return;
  document.documentElement.classList.toggle('dark', resolved === 'dark');
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<ThemeMode>(() => readMode() ?? 'dark');

  const setMode = useCallback((next: ThemeMode) => setModeState(next), []);
  const toggle = useCallback(
    () => setModeState(prev => (resolve(prev) === 'dark' ? 'light' : 'dark')),
    [],
  );

  // Apply + mirror on every mode change.
  useEffect(() => {
    const resolved = resolve(mode);
    applyTheme(resolved);
    try {
      localStorage.setItem(STORAGE_KEY, mode);
    } catch {
      /* private mode — ignore */
    }
  }, [mode]);

  // Follow the OS while mode is "system".
  useEffect(() => {
    if (mode !== 'system' || !window.matchMedia) return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const onChange = () => applyTheme(mq.matches ? 'dark' : 'light');
    mq.addEventListener('change', onChange);
    return () => mq.removeEventListener('change', onChange);
  }, [mode]);

  const value = useMemo<ThemeState>(
    () => ({ mode, resolved: resolve(mode), setMode, toggle }),
    [mode, setMode, toggle],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeState {
  return useContext(ThemeContext);
}