import { defineConfig } from '@playwright/test'

// Parity specs live in the MDD-conventional location (.mdd/tests/ui) and are
// linked from .mdd/trace.yml generated_ui_tests. This runner serves the
// mockups app so each spec can load /mockup/<slug> and assert the UIC- contract.
export default defineConfig({
  testDir: '../.mdd/tests/ui',
  testMatch: '**/*.spec.ts',
  use: { baseURL: 'http://localhost:4317' },
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:4317',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
})
