import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/health': 'http://localhost:3000',
      '/config': 'http://localhost:3000',
      '/maps': 'http://localhost:3000',
      '/rcon': 'http://localhost:3000',
    },
  },
})
