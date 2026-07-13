import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: './src/test/setup.ts',
    // Only our own tests. The default `**/node_modules/**` exclude does not match
    // the per-environment `node_modules.<env>` trees the justfile installs (see
    // `just fe-deps`), so without this vitest crawls into the dependency tree and
    // runs its tests (e.g. zod's 185 locale suites).
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    exclude: ['**/node_modules*/**', '**/dist/**', '**/e2e/**'],
  },
})
