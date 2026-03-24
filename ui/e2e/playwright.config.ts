import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright configuration for Molt Hub e2e tests.
 *
 * Assumes the Vite dev server is already running on 127.0.0.1:5173.
 * Run `npm run dev` in the ui/ directory before executing e2e tests.
 */
export default defineConfig({
  testDir: ".",
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: true,
  retries: 0,
  reporter: "list",

  use: {
    baseURL: "http://127.0.0.1:5173",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  /* Do NOT start the dev server automatically — caller is responsible. */
});
