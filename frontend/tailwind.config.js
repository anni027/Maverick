/** @type {import('tailwindcss').Config} */
export default {
  content: [
    './index.html',
    './src/**/*.{ts,tsx}',
  ],
  theme: {
    extend: {
      colors: {
        // Nexus keeps its cyan accent; everything else maps to the HSL token
        // system below so dark/light toggles are one variable swap.
        accent: {
          DEFAULT: 'hsl(var(--accent))',
          2: 'hsl(var(--accent-2))',
          dim: 'hsl(var(--accent-dim))',
          border: 'hsl(var(--accent-border))',
        },
      },
      fontFamily: {
        head: ['var(--font-head)', 'system-ui', 'sans-serif'],
        body: ['var(--font-body)', 'system-ui', 'sans-serif'],
        mono: ['var(--font-mono)', 'ui-monospace', 'monospace'],
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
          '0%, 100%': { boxShadow: '0 0 0 0 hsl(var(--accent-dim))' },
          '50%': { boxShadow: '0 0 24px 2px hsl(var(--accent-dim))' },
        },
        'status-strip': {
          '0%': { backgroundPosition: '-200% 0' },
          '100%': { backgroundPosition: '200% 0' },
        },
      },
      animation: {
        'streaming-sheen': 'streaming-sheen 2.4s linear infinite',
        'run-pulse': 'run-pulse 1.4s ease-in-out infinite',
        'goal-glow': 'goal-glow 2.6s ease-in-out infinite',
        'status-strip': 'status-strip 3s linear infinite',
      },
    },
  },
  plugins: [
    // `@container thread-composer` / `@container thread-viewport` queries.
    require('@tailwindcss/container-queries'),
  ],
};