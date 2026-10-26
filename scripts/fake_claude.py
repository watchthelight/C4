#!/usr/bin/env python3
# <wtl> project: c4 | file: scripts/fake_claude.py | author: watchthelight </wtl>
"""A stand-in for a running Claude Code session, for shell and manual checks of C4's discovery.

  python scripts/fake_claude.py --home <scratch home> --cwd <dir> [--session <uuid>] [--name <text>]
                                [--status busy|idle|waiting] [--waiting-for <text>]
                                [--timeline <steps.json>] [--crash] [--quiet]

The script registers itself the way Claude Code does: sessions/<pid>.json under --home, with its own
pid and process creation time, so C4's liveness check sees a live session. Then it waits. Ctrl+C,
SIGTERM or the end of its stdin (when stdin is a pipe) ends it. A clean end removes the record, as
Claude Code's exit does; --crash leaves the record behind so C4 sees a dead pid.

A timeline is a JSON list of steps run in order. Each has "at", seconds after start, and one action:
  {"at": 1.0, "status": "waiting", "waitingFor": "permission prompt"}
  {"at": 2.0, "name": "nightly build", "nameSource": "user"}
  {"at": 2.5, "transcript": {"type": "user", "uuid": "..."}}     appended to the main transcript
  {"at": 3.0, "agent": {"id": "a0000001", "meta": {"agentType": "Explore"}, "records": [...]}}
  {"at": 5.0, "exit": "clean"}  or  {"at": 5.0, "exit": "crash"}
After the last step the script keeps waiting unless a step ended it.

The script refuses the real Claude home. Point --home at a scratch directory; C4 reads it when
C4_CLAUDE_HOME names the same path. The same stand-in exists in Rust as crates/c4-testkit.
"""
import argparse
import json
import os
import signal
import sys
import threading
import time
import uuid
from pathlib import Path

VERSION = "2.1.299"  # the Claude Code version the record claims; the fixtures use the same one


def creation_time():
    """The process creation time in the shape Claude Code writes: a FILETIME string on Windows
    (verified), Unix seconds elsewhere (the real encoding there is not verified; C4 accepts both)."""
    if sys.platform == "win32":
        import ctypes
        import ctypes.wintypes as wt

        kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
        # Without these, the pseudo handle (-1) is truncated to 32 bits and the call fails.
        kernel32.GetCurrentProcess.restype = wt.HANDLE
        kernel32.GetProcessTimes.restype = wt.BOOL
        kernel32.GetProcessTimes.argtypes = [wt.HANDLE] + [ctypes.POINTER(wt.FILETIME)] * 4
        times = [wt.FILETIME() for _ in range(4)]
        if kernel32.GetProcessTimes(kernel32.GetCurrentProcess(), *(ctypes.byref(t) for t in times)):
            created = times[0]
            return str((created.dwHighDateTime << 32) | created.dwLowDateTime)
        print("fake_claude: GetProcessTimes failed, writing Unix seconds instead", file=sys.stderr)
    elif sys.platform.startswith("linux"):
        try:
            stat = Path("/proc/self/stat").read_text(encoding="ascii")
            fields = stat[stat.rindex(")") + 2:].split()
            start_ticks = int(fields[19])
            btime = next(int(line.split()[1]) for line in Path("/proc/stat").read_text(encoding="ascii").splitlines()
                         if line.startswith("btime"))
            return btime + start_ticks // os.sysconf("SC_CLK_TCK")
        except (OSError, ValueError, StopIteration):
            pass
    return int(time.time())  # macOS and fallbacks: the process started a moment ago; C4 allows 2 s


def encode_cwd(cwd: str) -> str:
    """Claude Code's project directory name: every character that is not an ASCII letter or digit is a dash."""
    return "".join(c if c.isascii() and c.isalnum() else "-" for c in cwd)


def real_home() -> Path:
    config = os.environ.get("CLAUDE_CONFIG_DIR")
    return Path(config) if config else Path.home() / ".claude"


class FakeClaude:
    def __init__(self, args):
        self.home = Path(args.home).resolve()
        self.cwd = args.cwd
        self.session = args.session or str(uuid.uuid4())
        self.name = args.name
        self.name_source = args.name_source
        self.status = args.status
        self.waiting_for = args.waiting_for
        self.crash = args.crash
        self.quiet = args.quiet
        self.pid = os.getpid()
        self.started_at = int(time.time() * 1000)
        self.proc_start = creation_time()
        self.record_path = self.home / "sessions" / f"{self.pid}.json"
        self.transcript_path = self.home / "projects" / encode_cwd(self.cwd) / f"{self.session}.jsonl"
        self.ended = threading.Event()

    def say(self, text):
        if not self.quiet:
            print(f"fake_claude[{self.pid}] {text}", file=sys.stderr, flush=True)

    def record(self):
        now = int(time.time() * 1000)
        record = {
            "pid": self.pid,
            "sessionId": self.session,
            "cwd": self.cwd,
            "startedAt": self.started_at,
            "procStart": self.proc_start,
            "version": VERSION,
            "peerProtocol": 1,
            "peerFeatures": ["notify_idle", "artifact_yield"],
            "kind": "interactive",
            "entrypoint": "cli",
            "updatedAt": now,
            "status": self.status,
            "statusUpdatedAt": now,
        }
        if self.name:
            record["name"] = self.name
            record["nameSource"] = self.name_source
            record["nameSince"] = self.started_at
        if self.waiting_for:
            record["waitingFor"] = self.waiting_for
        return record

    def write_record(self):
        self.record_path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.record_path.with_name(self.record_path.name + ".tmp")
        tmp.write_text(json.dumps(self.record(), indent=2) + "\n", encoding="utf-8")
        os.replace(tmp, self.record_path)

    def append_transcript(self, obj):
        self.transcript_path.parent.mkdir(parents=True, exist_ok=True)
        with self.transcript_path.open("a", encoding="utf-8") as f:
            f.write(json.dumps(obj) + "\n")

    def write_agent(self, spec):
        agent_id = spec["id"]
        sub = self.transcript_path.with_suffix("") / "subagents"
        sub.mkdir(parents=True, exist_ok=True)
        path = sub / f"agent-{agent_id}.jsonl"
        with path.open("a", encoding="utf-8") as f:
            for obj in spec.get("records", []):
                f.write(json.dumps(obj) + "\n")
        if "meta" in spec:
            (sub / f"agent-{agent_id}.meta.json").write_text(json.dumps(spec["meta"], indent=2) + "\n",
                                                               encoding="utf-8")

    def apply(self, step):
        if "status" in step:
            self.status = step["status"]
            self.waiting_for = step.get("waitingFor")
            self.write_record()
            self.say(f"status {self.status}" + (f" ({self.waiting_for})" if self.waiting_for else ""))
        elif "name" in step:
            self.name = step["name"]
            self.name_source = step.get("nameSource", "user")
            self.write_record()
            self.say(f"name {self.name!r}")
        elif "transcript" in step:
            self.append_transcript(step["transcript"])
            self.say("transcript record appended")
        elif "agent" in step:
            self.write_agent(step["agent"])
            self.say(f"agent file {step['agent']['id']}")
        elif "exit" in step:
            self.crash = step["exit"] == "crash"
            self.say("crash" if self.crash else "exit")
            self.ended.set()
        else:
            self.say(f"unknown step ignored: {step}")

    def run(self, timeline):
        self.write_record()
        self.say(f"registered session {self.session} in {self.record_path}")
        signal.signal(signal.SIGTERM, lambda *_: self.ended.set())
        if not sys.stdin.isatty():
            threading.Thread(target=self._watch_stdin, daemon=True).start()
        start = time.monotonic()
        try:
            for step in sorted(timeline, key=lambda s: float(s.get("at", 0))):
                wait = start + float(step.get("at", 0)) - time.monotonic()
                if wait > 0 and self.ended.wait(wait):
                    break
                if self.ended.is_set():
                    break
                self.apply(step)
            while not self.ended.is_set():
                self.ended.wait(3600)
        except KeyboardInterrupt:
            pass
        finally:
            if not self.crash:
                try:
                    self.record_path.unlink()
                except FileNotFoundError:
                    pass
            self.say("record left behind" if self.crash else "record removed")

    def _watch_stdin(self):
        try:
            while sys.stdin.buffer.read(1024):
                pass
        except (OSError, ValueError):
            pass
        self.ended.set()


def main(argv=None):
    parser = argparse.ArgumentParser(description="A stand-in for a running Claude Code session.")
    parser.add_argument("--home", required=True, help="the scratch Claude home to register in")
    parser.add_argument("--cwd", required=True, help="the working directory the record names")
    parser.add_argument("--session", help="the session id (default: a fresh uuid4)")
    parser.add_argument("--name", help="a display name")
    parser.add_argument("--name-source", default="user", choices=["user", "auto", "derived"])
    parser.add_argument("--status", default="idle", choices=["busy", "idle", "waiting"])
    parser.add_argument("--waiting-for", help="why it waits, with --status waiting")
    parser.add_argument("--timeline", help="a JSON file with the steps described in the module docstring")
    parser.add_argument("--crash", action="store_true", help="leave the record behind on exit")
    parser.add_argument("--quiet", action="store_true", help="print nothing on stderr")
    args = parser.parse_args(argv)

    home = Path(args.home).resolve()
    if home == real_home().resolve():
        print("fake_claude: refusing to write into the real Claude home; pass a scratch directory", file=sys.stderr)
        return 2
    timeline = []
    if args.timeline:
        timeline = json.loads(Path(args.timeline).read_text(encoding="utf-8"))
        if not isinstance(timeline, list):
            print("fake_claude: the timeline must be a JSON list of steps", file=sys.stderr)
            return 2
    FakeClaude(args).run(timeline)
    return 0


if __name__ == "__main__":
    sys.exit(main())
