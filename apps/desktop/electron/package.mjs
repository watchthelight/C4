// <wtl> project: c4 | file: apps/desktop/electron/package.mjs | author: watchthelight </wtl>
// `pnpm package`: the installers for this platform under release/; `pnpm package:dir`: the unpacked
// app alone. The core and the shim must already sit under resources/bin, where scripts/stage_bin.py
// puts them, or nothing is built: a package without them would open to an error box. Every
// argument goes on to electron-builder as it is.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const app = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const exe = process.platform === "win32" ? ".exe" : "";
const missing = ["c4-cli", "c4-shim"]
  .map((name) => path.join(app, "resources", "bin", `${name}${exe}`))
  .filter((file) => !existsSync(file));
if (missing.length > 0) {
  process.stderr.write(
    `package: missing ${missing.join(" and ")}\n` +
      "package: run python scripts/stage_bin.py from the repository root first " +
      "(the release build; --profile debug for the debug build)\n",
  );
  process.exit(1);
}

const builder = createRequire(import.meta.url).resolve("electron-builder/cli.js");
const result = spawnSync(process.execPath, [builder, ...process.argv.slice(2)], {
  cwd: app,
  stdio: "inherit",
});
process.exit(result.status ?? 1);
