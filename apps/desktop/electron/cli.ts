// <wtl> project: c4 | file: apps/desktop/electron/cli.ts | author: watchthelight </wtl>
// `c4` with a command is the command line: the arguments go to c4-cli as they are and no window
// opens. This is the pure part of that rule, so a test runs it without Electron.

/** The flags that are a command on their own. */
const FLAGS: ReadonlySet<string> = new Set(["--version", "-V", "--help", "-h"]);

/**
 * The arguments to relay, or null when the window should open. Electron puts the executable in
 * front of the user's arguments, and the app's path as well when it was started through the
 * `electron` binary itself (`electron .`), which is what `defaultApp` says. A command line that
 * holds nothing but switches opens the window: Chromium's and Electron's own switches stay in
 * argv (`--remote-debugging-port`, `--inspect`, the `--remote-debugging-pipe` a test driver adds),
 * and older macOS releases added a process serial number (`-psn_...`) when an app was opened from
 * the Finder. A word (`sessions`, `doctor`, `install`) or one of the help and version flags is a
 * command, relayed whole.
 */
export function relayArgs(argv: readonly string[], defaultApp: boolean): string[] | null {
  const args = argv.slice(defaultApp ? 2 : 1);
  const command = args.some((a) => !a.startsWith("-") || FLAGS.has(a));
  return command ? args : null;
}
