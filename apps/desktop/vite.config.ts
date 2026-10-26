// <wtl> project: c4 | file: apps/desktop/vite.config.ts | author: watchthelight </wtl>
// Vite serves the window during `tauri dev` on a fixed port and builds `dist/` for the app;
// vitest runs the unit tests in jsdom.

import { defineConfig } from "vitest/config";

export default defineConfig({
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: ["es2022", "chrome110", "safari15"],
    sourcemap: false,
  },
  test: {
    environment: "jsdom",
    include: ["tests/**/*.test.ts"],
    // vitest blanks every stylesheet it is not told to process; tokens.css is read as text by the
    // list test that checks the row heights against the TypeScript constants
    css: { include: [/tokens\.css/] },
  },
});
