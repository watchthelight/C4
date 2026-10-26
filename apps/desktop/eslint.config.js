// <wtl> project: c4 | file: apps/desktop/eslint.config.js | author: watchthelight </wtl>
import js from "@eslint/js";
import { defineConfig, globalIgnores } from "eslint/config";
import tseslint from "typescript-eslint";

export default defineConfig([
  globalIgnores(["dist/", "src-tauri/", "node_modules/"]),
  js.configs.recommended,
  tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/consistent-type-imports": "error",
      "no-console": "error",
    },
  },
]);
