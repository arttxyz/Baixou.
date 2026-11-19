/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      fontFamily: {
        sans: ['"Plus Jakarta Sans"', 'sans-serif'],
        mono: ['"JetBrains Mono"', 'monospace'],
      },
      colors: {
        ink: '#07090d',
        panel: '#0d1118',
        mint: '#9df7c4',
        electric: '#8fa8ff',
      },
      boxShadow: {
        glow: '0 0 0 1px rgba(157,247,196,.16), 0 20px 80px rgba(0,0,0,.28)',
      },
    },
  },
  plugins: [],
}
