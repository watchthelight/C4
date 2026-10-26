// <wtl> project: c4 | file: apps/desktop/e2e/playwright.config.ts | author: watchthelight </wtl>
// Playwright over the packaged app: the unpacked build under release/ is started through
// Playwright's Electron launcher, so every check runs against the real main process, the real core
// from the app's own resources and the real page. `pnpm package:dir` writes that build
// (docs/BUILDING.md); `pnpm e2e` runs this. No browser download is involved.

import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: /.*\.spec\.ts$/,
  outputDir: "../test-results",
  timeout: 90_000,
  expect: { timeout: 20_000 },
  fullyParallel: false,
  workers: 1,
  retries: 0,
  forbidOnly: process.env.CI !== undefined,
  reporter: process.env.CI !== undefined ? [["list"], ["github"]] : [["list"]],
});
