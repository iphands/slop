import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // The Rust binary embeds dist/ at compile time; keep asset names hashed so
  // they can be served immutable while index.html stays no-cache.
  build: { outDir: 'dist', emptyOutDir: true },
  server: { proxy: { '/api': 'http://localhost:8081' } },
  test: { environment: 'node', include: ['src/**/*.test.ts'] },
});
