import { defineConfig, devices } from '@playwright/test'

// End-to-end tests run the real Rust server on a fresh data directory, with
// Vite in front of it, exactly as in development. They use their own ports,
// so a development server on the usual ones can keep running.
export default defineConfig({
  testDir: 'e2e',
  timeout: 60_000,
  fullyParallel: false,
  workers: 1,
  use: { baseURL: 'http://localhost:5199', trace: 'retain-on-failure' },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: [
    {
      command:
        'rm -rf ../target/e2e-data && cargo run -q -p loquaris-server',
      env: {
        CARGO_TARGET_DIR: '../target/e2e',
        LOQ_DATA_DIR: '../target/e2e-data',
        LOQ_BIND: '127.0.0.1:8099',
        LOQ_PUBLIC_URL: 'http://localhost:5199',
        LOQ_REGISTRATION: 'open',
      },
      url: 'http://localhost:8099/api/config',
      timeout: 600_000,
      reuseExistingServer: false,
    },
    {
      command: 'npx vite --port 5199 --strictPort',
      env: { LOQ_API: 'http://localhost:8099' },
      url: 'http://localhost:5199',
      timeout: 120_000,
      reuseExistingServer: false,
    },
  ],
})
