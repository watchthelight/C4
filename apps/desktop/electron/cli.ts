// <wtl> project: c4 | file: apps/desktop/electron/cli.ts | author: watchthelight </wtl>
// `c4` with a command is the command line: the arguments go to c4-cli from the command on and no
// window opens. This is the pure part of that rule, so a test runs it without Electron.

/** The flags that are a command on their own. */
const FLAGS: ReadonlySet<string> = new Set(["--version", "-V", "--help", "-h"]);

/** The core's own options that may stand before the command (`--log debug sessions`); each takes a value. */
const CORE_OPTIONS: readonly string[] = ["--log"];

/**
 * The arguments to relay, or null when the window should open. Electron puts the executable in
 * front of the user's arguments, and the app's path as well when it was started through the
 * `electron` binary itself (`electron .`), which is what `defaultApp` says. A word (`sessions`,
 * `doctor`, `install`) or one of the help and version flags is a command, relayed with everything
 * after it. The switches before it are the window's: Chromium's and Electron's own
 * (`--no-sandbox`, `--remote-debugging-port`, `--inspect`, the `--remote-debugging-pipe` a test
 * driver adds, the `-psn_...` older macOS releases added when an app was opened from the Finder)
 * have been read by Chromium already and the core would refuse them, so they are dropped; the
 * core's own options, `--log` with its value, go along. A command line without a command opens
 * the window.
 */
export function relayArgs(argv: readonly string[], defaultApp: boolean): string[] | null {
  const args = argv.slice(defaultApp ? 2 : 1);
  const relay: string[] = [];
  let i = 0;
  while (i < args.length) {
    const arg = args[i];
    if (arg === undefined || !arg.startsWith("-") || FLAGS.has(arg)) {
      break;
    }
    i += 1;
    const option = CORE_OPTIONS.find((o) => arg === o || arg.startsWith(`${o}=`));
    if (option === undefined) {
      continue;
    }
    relay.push(arg);
    const value = args[i];
    if (arg === option && value !== undefined) {
      relay.push(value);
      i += 1;
    }
  }
  if (i >= args.length) {
    return null;
  }
  relay.push(...args.slice(i));
  return relay;
}
