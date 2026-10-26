// <wtl> project: c4 | file: apps/desktop/electron/log.ts | author: watchthelight </wtl>
// Lines on stderr with a timestamp and a level, beside the core's own. C4_LOG names the lowest
// level that prints (error, warn, info or debug); info is the default.

const LEVELS = { error: 0, warn: 1, info: 2, debug: 3 } as const;

export type Level = keyof typeof LEVELS;

function wanted(): Level {
  const raw = process.env.C4_LOG;
  return raw !== undefined && Object.hasOwn(LEVELS, raw) ? (raw as Level) : "info";
}

const threshold = LEVELS[wanted()];

/** One line, such as `2026-10-26T12:00:00.000Z warn  c4 the core exited {"code":1}`. */
export function log(level: Level, what: string, fields?: Record<string, unknown>): void {
  if (LEVELS[level] > threshold) {
    return;
  }
  const tail = fields === undefined ? "" : ` ${JSON.stringify(fields)}`;
  process.stderr.write(`${new Date().toISOString()} ${level.padEnd(5)} c4 ${what}${tail}\n`);
}
