/** @type {import('tailwindcss').Config} */
export default {
  darkMode: ['class'],
  // Preflight stays OFF: nexus's element-level CSS (button/input/scrollbar)
  // predates Tailwind and must not be reset underneath the legacy UI. New
  // components opt into utilities explicitly.
  core: { preflight: false },
  content: [
    './index.html',
    './src/**/*.{ts,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        // shadcn-style utility names mapped over nexus's legacy CSS vars, so
        // every theme swap (dark/light) is a single variable swap. NOTE: nexus
        // vars are raw hex/rgba (NOT HSL triples) — never wrap them in hsl().
        // Nexus semantics win on the shadcn collisions: `--accent` is the cyan
        // accent (not shadcn's hover bg) and `--muted` is nexus's muted TEXT
        // color (not shadcn's muted bg) — hence `muted.foreground` below.
        background: 'var(--bg)',
        foreground: 'var(--text)',
        card: { DEFAULT: 'var(--panel)', foreground: 'var(--text)' },
        popover: { DEFAULT: 'var(--panel-2)', foreground: 'var(--text)' },
        primary: { DEFAULT: 'var(--accent)', foreground: '#000000' },
        secondary: { DEFAULT: 'var(--panel-3)', foreground: 'var(--text)' },
        muted: { DEFAULT: 'var(--panel-2)', foreground: 'var(--muted)' },
        accent: {
          DEFAULT: 'var(--accent)',
          2: 'var(--accent-2)',
          dim: 'var(--accent-dim)',
          border: 'var(--accent-border)',
        },
        destructive: { DEFAULT: 'var(--danger)', foreground: 'var(--danger-solid-text)' },
        border: 'var(--line)',
        input: 'var(--line)',
        ring: 'var(--accent)',
        sidebar: {
          DEFAULT: 'var(--panel-2)',
          foreground: 'var(--text)',
          accent: 'var(--row-hover)',
          border: 'var(--line)',
        },
      },
      fontFamily: {
        head: ['var(--font-head)', 'system-ui', 'sans-serif'],
        body: ['var(--font-body)', 'system-ui', 'sans-serif'],
        mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
      },
      borderRadius: {
        lg: 'var(--radius)',
        md: 'calc(var(--radius) - 2px)',
        sm: 'calc(var(--radius) - 4px)',
      },
      keyframes: {
        'streaming-sheen': {
          '0%': { backgroundPosition: '200% 0' },
          '100%': { backgroundPosition: '-200% 0' },
        },
        'run-pulse': {
          '0%, 100%': { opacity: '0.55', transform: 'scale(1)' },
          '50%': { opacity: '1', transform: 'scale(1.15)' },
        },
        'goal-glow': {
          '0%, 100%': { boxShadow: '0 0 0 0 var(--accent-dim)' },
          '50%': { boxShadow: '0 0 24px 2px var(--accent-dim)' },
        },
        'status-strip': {
          '0%': { backgroundPosition: '-200% 0' },
          '100%': { backgroundPosition: '200% 0' },
        },
        'model-pill-enter': {
          from: { transform: 'scale(0.9074)' },
          to: { transform: 'scale(1)' },
        },
      },
      animation: {
        'streaming-sheen': 'streaming-sheen 2.4s linear infinite',
        'run-pulse': 'run-pulse 1.4s ease-in-out infinite',
        'goal-glow': 'goal-glow 2.6s ease-in-out infinite',
        'status-strip': 'status-strip 3s linear infinite',
        'model-pill-enter': 'model-pill-enter 210ms cubic-bezier(0.2, 0.8, 0.2, 1) both',
      },
    },
  },
  plugins: [
    // `@container thread-composer` / `@container thread-viewport` queries.
    require('@tailwindcss/container-queries'),
  ],
};