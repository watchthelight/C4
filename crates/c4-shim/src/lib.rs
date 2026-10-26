// <wtl> project: c4 | file: crates/c4-shim/src/lib.rs | author: watchthelight </wtl>
//! The sidecar behind `~/.c4/bin/c4-shim`. Claude Code runs it in place of the status line command
//! and as a hook; it copies the payload on stdin to C4 and, for the status line, runs the original
//! command with that same stdin, passing its output and exit code through untouched.
//!
//! Standard library only. The binary starts on every status line refresh and every hook event, so it
//! stays small and resolves nothing at startup. The rules it shares with the rest of C4 (where the
//! home is, how the endpoint is named, the shape of a line) are repeated here in a few lines each and
//! held in step with `c4-core` by a test.
//!
//! Rules it keeps whatever happens: the original command runs whenever it can be started and its exit
//! code becomes the shim's; `hook` prints nothing and exits 0; a payload C4 cannot take right now is
//! written to the spool rather than lost; and no delivery holds Claude Code past a short budget.

use std::ffi::{OsStr, OsString};
use std::fmt::{self, Write as _};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Overrides the C4 home. The same variable `c4-core` reads.
pub const HOME_ENV: &str = "C4_HOME";
/// Turns on the run log under `<home>/logs/shim.log` when set to anything but an empty value.
pub const LOG_ENV: &str = "C4_SHIM_LOG";
/// The Windows pipe prefix; the hash of the home follows. `LOCAL\` is what Claude Code's own pipes
/// use; the server's DACL, not the prefix, keeps other users out.
pub const PIPE_PREFIX: &str = r"\\.\pipe\LOCAL\c4-ingest-";
/// The socket file under the home on Unix.
pub const SOCKET_NAME: &str = "ingest.sock";
/// The largest payload the shim copies to C4. A larger one still reaches the original command whole;
/// it is only not teed, because nothing Claude Code sends comes close and a runaway producer must
/// not fill memory or the spool.
pub const MAX_TEE_BYTES: usize = 1 << 20;
/// The size a spool file may reach before further lines for that day are dropped.
pub const SPOOL_CAP_BYTES: u64 = 20 << 20;
/// How long a hook waits for its delivery before exiting anyway, measured from the moment the
/// payload was read. Under the 1 s timeout of the one synchronous hook (`SessionEnd`) and far under
/// the 5 s of the asynchronous ones, so Claude Code never has to kill the shim.
pub const HOOK_BUDGET: Duration = Duration::from_millis(900);
/// How long the status line waits for a delivery that is still running once the original command
/// has finished, measured from the moment the payload was read. The delivery runs beside the
/// command, which takes longer than this on its own, so the wait is rarely needed at all.
pub const STATUSLINE_GRACE: Duration = Duration::from_millis(250);
/// Socket write timeout, so a server that accepted and stopped reading cannot hold the shim.
#[cfg(unix)]
const SEND_BUDGET: Duration = Duration::from_millis(250);

/// What kind of payload a line carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// The JSON Claude Code writes to the status line command.
    Statusline,
    /// The JSON Claude Code writes to a hook.
    Hook,
}

impl Kind {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Statusline => "statusline",
            Self::Hook => "hook",
        }
    }
}

/// The root C4 writes under. The same rule as `c4_core::home::C4Home`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Home {
    root: PathBuf,
}

impl Home {
    /// Resolve from the process environment: `C4_HOME`, else `.c4` under the user's home.
    #[must_use]
    pub fn resolve() -> Self {
        Self::resolve_from(std::env::var_os(HOME_ENV).as_deref(), &user_home())
    }

    /// The resolution rule with its inputs made explicit. An empty override counts as unset.
    #[must_use]
    pub fn resolve_from(c4_override: Option<&OsStr>, user_home: &Path) -> Self {
        if let Some(p) = c4_override.filter(|p| !p.is_empty()) {
            return Self::at(p);
        }
        Self::at(user_home.join(".c4"))
    }

    /// A home at a known path.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `spool/`: lines C4 could not take, one file per UTC day.
    #[must_use]
    pub fn spool_dir(&self) -> PathBuf {
        self.root.join("spool")
    }

    /// `logs/`.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// `ingest.sock`.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.root.join(SOCKET_NAME)
    }

    /// The Windows pipe name for this home.
    #[must_use]
    pub fn pipe_name(&self) -> String {
        format!(
            "{PIPE_PREFIX}{:016x}",
            fnv1a64(home_key(&self.root).as_bytes())
        )
    }

    /// Where the shim connects on this platform.
    #[must_use]
    pub fn endpoint(&self) -> String {
        if cfg!(windows) {
            self.pipe_name()
        } else {
            self.socket_path().to_string_lossy().into_owned()
        }
    }
}

/// The user's home directory: `HOME`, else `USERPROFILE`, else the current directory.
#[must_use]
pub fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|p| !p.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|p| !p.is_empty()))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// The home path as the pipe name hashes it: forward slashes, lowercase, no trailing separator.
#[must_use]
pub fn home_key(root: &Path) -> String {
    let text = root.to_string_lossy().replace('\\', "/").to_lowercase();
    text.trim_end_matches('/').to_string()
}

/// FNV-1a, 64 bits.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// One line of the wire format: `{"kind":..,"recv_at":<ms>,"pid":<n>,"payload":..}` and a newline.
/// A payload that is a JSON object or array travels as it is, with any newline between its tokens
/// turned into a space so the line stays one line; anything else travels as `{"raw": "<text>"}`.
#[must_use]
pub fn envelope(kind: Kind, recv_at: u64, pid: u32, payload: &[u8]) -> String {
    let text = String::from_utf8_lossy(payload);
    let trimmed = text.trim();
    let looks_like_json = (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'));
    let body = if looks_like_json {
        trimmed.replace(['\n', '\r'], " ")
    } else {
        format!("{{\"raw\":\"{}\"}}", json_escape(trimmed))
    };
    let kind = kind.as_str();
    format!("{{\"kind\":\"{kind}\",\"recv_at\":{recv_at},\"pid\":{pid},\"payload\":{body}}}\n")
}

/// Escape `text` for the inside of a JSON string literal.
#[must_use]
pub fn json_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

/// The UTC civil date of a moment given in seconds since the epoch, as `YYYY-MM-DD`.
#[must_use]
pub fn utc_date(secs_since_epoch: u64) -> String {
    // Days to a civil date, after Howard Hinnant's algorithm.
    let days = i64::try_from(secs_since_epoch / 86_400).unwrap_or(i64::MAX / 4);
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Read up to `cap` bytes. The flag is false when more was waiting; the rest is still unread, so the
/// caller can stream it on. The byte that proved the overflow stays with the payload.
pub fn read_capped<R: Read>(input: &mut R, cap: usize) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let limit = u64::try_from(cap).unwrap_or(u64::MAX).saturating_add(1);
    // A read error means the producer went away; whatever arrived is the payload.
    let _ = input.take(limit).read_to_end(&mut buf);
    let complete = buf.len() <= cap;
    (buf, complete)
}

/// What became of one line.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// C4 took it over the socket.
    Sent,
    /// No C4 was listening; the line waits in this spool file.
    Spooled(PathBuf),
    /// Neither worked; the reason.
    Dropped(String),
    /// The shim stopped waiting for a delivery that was still running.
    Abandoned,
    /// Nothing was sent, on purpose.
    Skipped(&'static str),
}

impl fmt::Display for Delivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sent => f.write_str("sent"),
            Self::Spooled(path) => write!(f, "spooled {}", path.display()),
            Self::Dropped(why) => write!(f, "dropped: {why}"),
            Self::Abandoned => f.write_str("abandoned"),
            Self::Skipped(why) => write!(f, "skipped: {why}"),
        }
    }
}

/// Send `line` to C4, or spool it.
pub fn deliver(home: &Home, line: &str) -> Delivery {
    if send(home, line).is_ok() {
        return Delivery::Sent;
    }
    match spool_with_cap(home, line, SPOOL_CAP_BYTES) {
        Ok(path) => Delivery::Spooled(path),
        Err(e) => Delivery::Dropped(e.to_string()),
    }
}

#[cfg(windows)]
fn send(home: &Home, line: &str) -> io::Result<()> {
    // A pipe that is not there fails at once; a server that is there reads the line and the data
    // outlives this handle, so there is nothing to wait for.
    let mut pipe = OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.pipe_name())?;
    pipe.write_all(line.as_bytes())?;
    pipe.flush()
}

#[cfg(unix)]
fn send(home: &Home, line: &str) -> io::Result<()> {
    use std::os::unix::net::UnixStream;
    let mut stream = UnixStream::connect(home.socket_path())?;
    stream.set_write_timeout(Some(SEND_BUDGET))?;
    stream.write_all(line.as_bytes())?;
    stream.flush()
}

#[cfg(not(any(windows, unix)))]
fn send(_home: &Home, _line: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "no local socket on this platform",
    ))
}

/// Append `line` to today's spool file, unless that would take the file past `cap` bytes.
pub fn spool_with_cap(home: &Home, line: &str, cap: u64) -> io::Result<PathBuf> {
    let dir = home.spool_dir();
    fs::create_dir_all(&dir)?;
    restrict(home.root());
    restrict(&dir);
    let path = dir.join(format!("{}.jsonl", utc_date(now_secs())));
    let used = fs::metadata(&path).map_or(0, |m| m.len());
    if used.saturating_add(line.len() as u64) > cap {
        return Err(io::Error::other("the spool file for today is full"));
    }
    let mut file = open_private_append(&path)?;
    file.write_all(line.as_bytes())?;
    file.flush()?;
    Ok(path)
}

/// Keep a directory to its owner where the platform has modes. Best effort.
fn restrict(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

fn open_private_append(path: &Path) -> io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// A delivery running on its own thread, so the shim can stop waiting for it.
pub struct Tee {
    rx: mpsc::Receiver<Delivery>,
    started: Instant,
}

/// Start delivering `payload` as a `kind` line stamped `recv_at`.
#[must_use]
pub fn start_tee(home: Home, kind: Kind, payload: Vec<u8>, recv_at: u64) -> Tee {
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    let pid = std::process::id();
    let worker = tx.clone();
    let spawned = thread::Builder::new()
        .name("c4-shim-tee".into())
        .spawn(move || {
            let line = envelope(kind, recv_at, pid, &payload);
            let _ = worker.send(deliver(&home, &line));
        });
    if spawned.is_err() {
        let _ = tx.send(Delivery::Dropped(
            "could not start the delivery thread".into(),
        ));
    }
    Tee { rx, started }
}

impl Tee {
    /// The result, or `Abandoned` once `budget` has passed since the start without one.
    #[must_use]
    pub fn wait(self, budget: Duration) -> Delivery {
        let left = budget.saturating_sub(self.started.elapsed());
        match self.rx.recv_timeout(left) {
            Ok(delivery) => delivery,
            Err(mpsc::RecvTimeoutError::Timeout) => Delivery::Abandoned,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Delivery::Dropped("the delivery thread ended without a result".into())
            }
        }
    }
}

/// Tee stdin if it is worth teeing: complete, under the cap and not blank.
fn tee_stdin(home: &Home, kind: Kind, bytes: &[u8], complete: bool, recv_at: u64) -> Option<Tee> {
    if !complete {
        return None;
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    Some(start_tee(home.clone(), kind, bytes.to_vec(), recv_at))
}

fn skipped_because(complete: bool) -> Delivery {
    if complete {
        Delivery::Skipped("empty payload")
    } else {
        Delivery::Skipped("payload over the tee cap")
    }
}

/// `c4-shim hook`: tee stdin, print nothing, exit 0.
pub fn run_hook(home: &Home) -> i32 {
    let recv_at = now_ms();
    let (bytes, complete) = read_capped(&mut io::stdin().lock(), MAX_TEE_BYTES);
    if !complete {
        // Take the rest so the producer never sees a broken pipe.
        let _ = io::copy(&mut io::stdin().lock(), &mut io::sink());
    }
    let delivery = tee_stdin(home, Kind::Hook, &bytes, complete, recv_at)
        .map_or_else(|| skipped_because(complete), |tee| tee.wait(HOOK_BUDGET));
    log(
        home,
        &format!(
            "hook bytes={} complete={complete} delivery={delivery}",
            bytes.len()
        ),
    );
    0
}

/// `c4-shim statusline -- <command> [args]`: tee stdin and run the command with the same stdin, its
/// stdout and stderr inherited and its exit code returned. A command that cannot start is reported
/// as the status line text, with exit 0, because a message is more use than an empty line.
pub fn run_statusline(home: &Home, command: &[OsString]) -> i32 {
    let recv_at = now_ms();
    let Some((program, args)) = command.split_first() else {
        return usage();
    };
    let (bytes, complete) = read_capped(&mut io::stdin().lock(), MAX_TEE_BYTES);
    let tee = tee_stdin(home, Kind::Statusline, &bytes, complete, recv_at);

    let spawned = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            println!(
                "c4-shim: could not start {}: {e}",
                program.to_string_lossy()
            );
            let delivery = tee.map_or_else(|| skipped_because(complete), |t| t.wait(HOOK_BUDGET));
            log(
                home,
                &format!(
                    "statusline bytes={} complete={complete} delivery={delivery} child=not-started",
                    bytes.len()
                ),
            );
            return 0;
        }
    };

    // Feed the command from another thread: it may write before it reads, and a pipe that fills
    // must not stop us from reaping it. Errors mean the command closed its stdin, which is its right.
    let feeder = child.stdin.take().map(|mut stdin| {
        let head = bytes.clone();
        thread::spawn(move || {
            let _ = stdin.write_all(&head);
            if !complete {
                let _ = io::copy(&mut io::stdin().lock(), &mut stdin);
            }
            let _ = stdin.flush();
        })
    });
    let code = match child.wait() {
        Ok(status) => exit_code(status),
        Err(_) => 1,
    };
    if let Some(feeder) = feeder {
        let _ = feeder.join();
    }
    let delivery = tee.map_or_else(|| skipped_because(complete), |t| t.wait(STATUSLINE_GRACE));
    log(
        home,
        &format!(
            "statusline bytes={} complete={complete} delivery={delivery} child={code}",
            bytes.len()
        ),
    );
    code
}

/// The command line: `statusline -- <command>`, `hook`, `endpoint`, `--version`.
pub fn run(args: &[OsString]) -> i32 {
    let Some((first, rest)) = args.split_first() else {
        return usage();
    };
    match first.to_str() {
        Some("statusline") => {
            let command = match rest.first() {
                Some(dashes) if dashes.as_os_str() == "--" => &rest[1..],
                _ => rest,
            };
            if command.is_empty() {
                return usage();
            }
            run_statusline(&Home::resolve(), command)
        }
        // A word after `hook` (an event name, say) is allowed and ignored.
        Some("hook") => run_hook(&Home::resolve()),
        Some("endpoint") => {
            println!("{}", Home::resolve().endpoint());
            0
        }
        Some("--version" | "-V" | "version") => {
            println!("c4-shim {}", env!("CARGO_PKG_VERSION"));
            0
        }
        _ => usage(),
    }
}

fn usage() -> i32 {
    eprintln!(
        "usage: c4-shim statusline -- <command> [args...]\n       c4-shim hook\n       c4-shim endpoint\n       c4-shim --version"
    );
    2
}

fn exit_code(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// One line per run in `<home>/logs/shim.log`, only when `C4_SHIM_LOG` is set. Never the payload.
fn log(home: &Home, text: &str) {
    if std::env::var_os(LOG_ENV).is_none_or(|v| v.is_empty()) {
        return;
    }
    let dir = home.logs_dir();
    let _ = fs::create_dir_all(&dir);
    restrict(home.root());
    restrict(&dir);
    if let Ok(mut file) = open_private_append(&dir.join("shim.log")) {
        let _ = writeln!(file, "{} {text}", now_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_rule_matches_c4_core() {
        for p in [
            r"C:\Users\Example\.c4",
            "c:/users/example/.c4/",
            "/home/example/.c4",
            "/tmp/c4 scratch/x",
        ] {
            let ours = Home::at(p);
            let theirs = c4_core::C4Home::at(p);
            assert_eq!(ours.pipe_name(), theirs.pipe_name(), "{p}");
            assert_eq!(ours.endpoint(), theirs.endpoint(), "{p}");
            assert_eq!(ours.socket_path(), theirs.socket_path());
            assert_eq!(ours.spool_dir(), theirs.spool_dir());
            assert_eq!(ours.logs_dir(), theirs.logs_dir());
        }
        assert_eq!(PIPE_PREFIX, c4_core::home::PIPE_PREFIX);
        assert_eq!(SOCKET_NAME, c4_core::home::SOCKET_NAME);
        assert_eq!(HOME_ENV, c4_core::home::HOME_ENV);
        let home = Path::new("/home/example");
        assert_eq!(
            Home::resolve_from(None, home).root(),
            c4_core::C4Home::resolve_from(None, home).root()
        );
        assert_eq!(
            Home::resolve_from(Some(OsStr::new("")), home).root(),
            Path::new("/home/example/.c4")
        );
        assert_eq!(
            Home::resolve_from(Some(OsStr::new("/x")), home).root(),
            Path::new("/x")
        );
    }

    #[test]
    fn fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn envelopes_are_one_json_line() {
        let line = envelope(
            Kind::Hook,
            1_792_972_800_123,
            42,
            b"{\n  \"a\": 1,\r\n  \"b\": \"x\"\n}\n",
        );
        assert_eq!(
            line,
            "{\"kind\":\"hook\",\"recv_at\":1792972800123,\"pid\":42,\"payload\":{   \"a\": 1,    \"b\": \"x\" }}\n"
        );
        let v: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(v["payload"]["b"], "x");

        let raw = envelope(
            Kind::Statusline,
            1,
            2,
            b"not json \"quoted\" \\ \t\x01 end\n",
        );
        assert_eq!(raw.matches('\n').count(), 1);
        let v: serde_json::Value = serde_json::from_str(raw.trim_end()).unwrap();
        assert_eq!(v["kind"], "statusline");
        assert_eq!(v["payload"]["raw"], "not json \"quoted\" \\ \t\u{1} end");

        let empty = envelope(Kind::Hook, 1, 2, b"  \n");
        let v: serde_json::Value = serde_json::from_str(empty.trim_end()).unwrap();
        assert_eq!(v["payload"]["raw"], "");

        let array = envelope(Kind::Hook, 1, 2, b"[1,2]");
        assert!(array.ends_with("\"payload\":[1,2]}\n"), "{array}");
    }

    #[test]
    fn dates_are_utc_civil_dates() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
        assert_eq!(utc_date(1_792_972_800), "2026-10-26");
        assert_eq!(utc_date(1_792_972_800 + 86_399), "2026-10-26");
        assert_eq!(utc_date(1_792_972_800 + 86_400), "2026-10-27");
    }

    #[test]
    fn reading_stops_at_the_cap_and_says_so() {
        let (bytes, complete) = read_capped(&mut &b"abc"[..], 8);
        assert_eq!((bytes.as_slice(), complete), (&b"abc"[..], true));
        let (bytes, complete) = read_capped(&mut &b"abcdefgh"[..], 8);
        assert!(complete);
        assert_eq!(bytes.len(), 8);
        let mut input = &b"abcdefghij"[..];
        let (bytes, complete) = read_capped(&mut input, 8);
        assert!(!complete);
        assert_eq!(bytes, b"abcdefghi", "the byte that proved the overflow");
        assert_eq!(input, b"j", "the rest is still unread");
    }

    #[test]
    fn a_payload_with_no_server_lands_in_the_spool() {
        let dir = tempfile::Builder::new()
            .prefix("c4-shim-")
            .tempdir()
            .unwrap();
        let home = Home::at(dir.path().join("c4"));
        let line = envelope(Kind::Hook, 5, 6, b"{\"a\":1}");
        let Delivery::Spooled(path) = deliver(&home, &line) else {
            panic!("expected a spooled line");
        };
        assert_eq!(path.parent(), Some(home.spool_dir().as_path()));
        assert_eq!(fs::read_to_string(&path).unwrap(), line);
        assert!(matches!(deliver(&home, &line), Delivery::Spooled(_)));
        assert_eq!(fs::read_to_string(&path).unwrap().lines().count(), 2);
        assert!(spool_with_cap(&home, &line, 10).is_err());
        let tee = start_tee(home.clone(), Kind::Hook, b"{\"b\":2}".to_vec(), 7);
        assert!(matches!(
            tee.wait(Duration::from_secs(5)),
            Delivery::Spooled(_)
        ));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(&home.spool_dir()), 0o700);
            assert_eq!(mode(home.root()), 0o700);
        }
    }
}
