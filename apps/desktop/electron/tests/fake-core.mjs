// <wtl> project: c4 | file: apps/desktop/electron/tests/fake-core.mjs | author: watchthelight </wtl>
// A stand-in for `c4-cli serve` with the same line protocol, for the sidecar tests: a hello line,
// then answers by id. Methods: echo (the params back), emit (an event line, then the answer),
// garbage (a line that is not JSON, then the answer), die (exit with params.code, no answer).
// `--protocol N` changes the hello line; `--linger` keeps the process alive after stdin closes.

import process from "node:process";
import { createInterface } from "node:readline";
import { setInterval } from "node:timers";

const args = process.argv.slice(2);
const at = args.indexOf("--protocol");
const protocol = at === -1 ? 1 : Number(args[at + 1]);
const linger = args.includes("--linger");

function out(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

out({ event: "hello", data: { protocol, schema: 1, version: "fake", pid: process.pid } });

const lines = createInterface({ input: process.stdin, crlfDelay: Infinity });
lines.on("line", (line) => {
  if (line.trim() === "") {
    return;
  }
  let request;
  try {
    request = JSON.parse(line);
  } catch (error) {
    out({ id: null, error: `not a request: ${error.message}` });
    return;
  }
  const { id = null, method, params = null } = request;
  switch (method) {
    case "echo":
      out({ id, ok: params });
      break;
    case "emit":
      out({ event: params.event, data: params.data });
      out({ id, ok: null });
      break;
    case "garbage":
      process.stdout.write("this is not json\n");
      out({ id, ok: null });
      break;
    case "die":
      process.exit(params.code);
      break;
    default:
      out({ id, error: `unknown method ${JSON.stringify(method)}` });
  }
});
lines.on("close", () => {
  if (!linger) {
    process.exit(0);
  }
});
if (linger) {
  setInterval(() => {}, 1000);
}
