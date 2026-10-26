// <wtl> project: c4 | file: apps/desktop/eslint.config.js | author: watchthelight </wtl>
import js from "@eslint/js";
import { defineConfig, globalIgnores } from "eslint/config";
import tseslint from "typescript-eslint";

export default defineConfig([
  globalIgnores(["dist/", "dist-electron/", "node_modules/"]),
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/consistent-type-imports": "error",
      "no-console": "error",
    },
  },
  {
    // plain Node scripts: the dev launcher and the stand-in core the sidecar tests run
    files: ["electron/**/*.mjs"],
    languageOptions: { globals: { fetch: "readonly" } },
  },
  {
    // a sandboxed preload script is CommonJS, so it takes the one import form that compiles to require
    files: ["electron/**/*.cts"],
    rules: {
      "@typescript-eslint/no-require-imports": ["error", { allowAsImport: true }],
    },
  },
]);
