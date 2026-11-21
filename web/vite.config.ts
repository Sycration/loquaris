import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

// The Rust server listens on 8080 in development (LOQ_API overrides it, as
// the end-to-end tests do); Vite proxies the API and the collaboration
// WebSocket to it.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      '/api': { target: process.env.LOQ_API ?? 'http://localhost:8080', ws: true },
    },
  },
  worker: { format: 'es' },
  test: {
    environment: 'jsdom',
    setupFiles: ['src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
  },
})
