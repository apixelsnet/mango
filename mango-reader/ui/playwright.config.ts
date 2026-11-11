import { defineConfig, devices } from '@playwright/test';

// The browser smoke test: the built UI, served by `vite preview`, against the stand-in API
// in e2e/mock.mjs. One worker, since the tests share the stand-in's record of changes.
export default defineConfig({
  testDir: 'e2e',
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['github'], ['list']] : 'list',
  use: {
    baseURL: 'http://127.0.0.1:4173',
    trace: 'retain-on-failure',
  },
  projects: [
    { name: 'desktop', use: { ...devices['Desktop Chrome'] } },
    { name: 'phone', use: { ...devices['Pixel 7'] } },
  ],
  webServer: [
    { command: 'node e2e/mock.mjs', url: 'http://127.0.0.1:8787/__writes', reuseExistingServer: !process.env.CI },
    { command: 'npm run build && npx vite preview --host 127.0.0.1 --port 4173 --strictPort', url: 'http://127.0.0.1:4173', reuseExistingServer: !process.env.CI },
  ],
});
