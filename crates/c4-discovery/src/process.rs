// <wtl> project: c4 | file: crates/c4-discovery/src/process.rs | author: watchthelight </wtl>
//! The process table: whether a pid still belongs to the process a registry record or a roster entry
//! describes. Pids are reused, so a live pid alone proves nothing; the creation time decides.

use crate::host::keeps_env_key;
use c4_core::ProcessRef;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// How far apart two readings of one creation time may be, in seconds. The registry writes a precise
/// `FILETIME`; the process table rounds to seconds.
pub const START_TOLERANCE_SECS: u64 = 2;

/// Seconds between the `FILETIME` epoch (1601) and the Unix epoch (1970).
const FILETIME_UNIX_OFFSET_SECS: u64 = 11_644_473_600;
/// `FILETIME` ticks per second (100 ns each).
const FILETIME_TICKS_PER_SEC: u64 = 10_000_000;
/// Below this a raw value cannot be a `FILETIME` of any date after 2000.
const FILETIME_MIN: u64 = 125_911_584_000_000_000;
/// Unix milliseconds for 2000-01-01 and 2100-01-01.
const UNIX_MS_MIN: u64 = 946_684_800_000;
const UNIX_MS_MAX: u64 = 4_102_444_800_000;
/// Unix seconds for 2000-01-01 and 2100-01-01.
const UNIX_SECS_MIN: u64 = 946_684_800;
const UNIX_SECS_MAX: u64 = 4_102_444_800;

/// What the probe learned about one pid.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ProcessInfo {
    /// The pid.
    pub pid: u32,
    /// The parent pid.
    pub parent: Option<u32>,
    /// When the process started, seconds since the Unix epoch.
    pub start_time_secs: Option<u64>,
    /// The process name (`claude.exe`, `node`).
    pub name: Option<String>,
    /// The executable path, when readable.
    pub exe: Option<PathBuf>,
    /// The working directory, when readable.
    pub cwd: Option<PathBuf>,
    /// The environment, reduced to the terminal host keys in [`crate::host::HOST_ENV_KEYS`].
    pub env: Vec<(String, String)>,
}

/// Looks up processes. The real one wraps `sysinfo`; tests use [`FakeProbe`].
pub trait ProcessProbe {
    /// Refresh and return one process, or `None` when no such pid exists.
    fn probe(&mut self, pid: u32) -> Option<ProcessInfo>;
}

/// The `sysinfo` probe. One targeted refresh per pid; nothing else in the process table is read.
pub struct SysinfoProbe {
    system: System,
}

impl Default for SysinfoProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SysinfoProbe {
    /// A probe with an empty process table.
    #[must_use]
    pub fn new() -> Self {
        Self {
            system: System::new(),
        }
    }
}

impl ProcessProbe for SysinfoProbe {
    fn probe(&mut self, pid: u32) -> Option<ProcessInfo> {
        let target = Pid::from_u32(pid);
        let kind = ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cwd(UpdateKind::OnlyIfNotSet)
            .with_environ(UpdateKind::OnlyIfNotSet);
        self.system
            .refresh_processes_specifics(ProcessesToUpdate::Some(&[target]), true, kind);
        let p = self.system.process(target)?;
        let env = p
            .environ()
            .iter()
            .filter_map(|kv| {
                let s = kv.to_string_lossy();
                let (k, v) = s.split_once('=')?;
                keeps_env_key(k).then(|| (k.to_string(), v.to_string()))
            })
            .collect();
        Some(ProcessInfo {
            pid,
            parent: p.parent().map(Pid::as_u32),
            start_time_secs: Some(p.start_time()),
            name: Some(p.name().to_string_lossy().into_owned()),
            exe: p.exe().map(Path::to_path_buf),
            cwd: p.cwd().map(Path::to_path_buf),
            env,
        })
    }
}

/// A scripted process table for tests.
#[derive(Clone, Debug, Default)]
pub struct FakeProbe {
    procs: HashMap<u32, ProcessInfo>,
}

impl FakeProbe {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or replace a process.
    pub fn insert(&mut self, info: ProcessInfo) -> &mut Self {
        self.procs.insert(info.pid, info);
        self
    }

    /// Remove a process, as if it exited.
    pub fn remove(&mut self, pid: u32) -> &mut Self {
        self.procs.remove(&pid);
        self
    }
}

impl ProcessProbe for FakeProbe {
    fn probe(&mut self, pid: u32) -> Option<ProcessInfo> {
        self.procs.get(&pid).cloned()
    }
}

/// The verdict on a pid.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    /// The pid exists and its creation time matches the record.
    Alive,
    /// The pid exists and looks like Claude Code, but no creation time could be compared. Treated as
    /// alive, marked as a guess.
    AliveUnverified,
    /// No such pid.
    Gone,
    /// The pid exists but belongs to another process now.
    Reused,
}

impl Liveness {
    /// True for the two verdicts that mean the process runs.
    #[must_use]
    pub fn is_alive(self) -> bool {
        matches!(self, Liveness::Alive | Liveness::AliveUnverified)
    }
}

/// A Windows `FILETIME` (100 ns ticks since 1601) as Unix seconds, or `None` for a value too small
/// to be one.
#[must_use]
pub fn filetime_to_unix_secs(ft: u64) -> Option<u64> {
    if ft < FILETIME_MIN {
        return None;
    }
    (ft / FILETIME_TICKS_PER_SEC).checked_sub(FILETIME_UNIX_OFFSET_SECS)
}

/// Compare a raw `procStart` value with a start time from the process table. The raw value is read
/// as a `FILETIME` when it is large enough (verified on Windows), else as Unix milliseconds, else as
/// Unix seconds (the macOS and Linux encodings are not verified; this is the ladder a test can pin
/// down). `None` means no reading of the raw value is a date at all.
#[must_use]
pub fn start_matches(expected_raw: u64, observed_secs: u64) -> Option<bool> {
    let close = |secs: u64| secs.abs_diff(observed_secs) <= START_TOLERANCE_SECS;
    if let Some(secs) = filetime_to_unix_secs(expected_raw) {
        return Some(close(secs));
    }
    if (UNIX_MS_MIN..UNIX_MS_MAX).contains(&expected_raw) {
        return Some(close(expected_raw / 1000));
    }
    if (UNIX_SECS_MIN..UNIX_SECS_MAX).contains(&expected_raw) {
        return Some(close(expected_raw));
    }
    None
}

/// True when the name or executable is one Claude Code runs as: the native `claude` binary, or
/// `node` and `bun` for the npm install.
#[must_use]
pub fn looks_like_claude(info: &ProcessInfo) -> bool {
    let stem = |s: &str| {
        let s = s.rsplit(['/', '\\']).next().unwrap_or(s);
        s.strip_suffix(".exe").unwrap_or(s).to_ascii_lowercase()
    };
    let names = [
        info.name.as_deref().map(stem),
        info.exe.as_deref().and_then(Path::to_str).map(stem),
    ];
    names
        .into_iter()
        .flatten()
        .any(|n| n == "claude" || n == "node" || n == "bun")
}

/// Decide whether `info` is the process `expected` describes.
#[must_use]
pub fn check_liveness(info: Option<&ProcessInfo>, expected: &ProcessRef) -> Liveness {
    let Some(info) = info else {
        return Liveness::Gone;
    };
    let compared = match (expected.proc_start, info.start_time_secs) {
        (Some(raw), Some(observed)) => start_matches(raw, observed),
        _ => None,
    };
    match compared {
        Some(true) => Liveness::Alive,
        Some(false) => Liveness::Reused,
        None if looks_like_claude(info) => Liveness::AliveUnverified,
        None => Liveness::Reused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime_conversion() {
        // The fixtures' FILETIME, a date in late October 2026.
        assert_eq!(
            filetime_to_unix_secs(134_373_000_000_000_000),
            Some(1_792_826_400)
        );
        assert_eq!(filetime_to_unix_secs(5), None);
        assert_eq!(
            start_matches(134_373_000_000_000_000, 1_792_826_401),
            Some(true)
        );
        assert_eq!(
            start_matches(134_373_000_000_000_000, 1_792_826_403),
            Some(false)
        );
        assert_eq!(start_matches(1_792_826_400_000, 1_792_826_400), Some(true));
        assert_eq!(start_matches(1_792_826_400, 1_792_826_402), Some(true));
        assert_eq!(start_matches(1_792_826_400, 1_792_826_403), Some(false));
        assert_eq!(start_matches(42, 1_792_826_400), None);
    }

    fn info(pid: u32, start: Option<u64>, name: &str) -> ProcessInfo {
        ProcessInfo {
            pid,
            start_time_secs: start,
            name: Some(name.into()),
            ..ProcessInfo::default()
        }
    }

    #[test]
    fn liveness_verdicts() {
        let expected = ProcessRef::new(41001, 134_373_000_000_000_000);
        assert_eq!(check_liveness(None, &expected), Liveness::Gone);
        let alive = info(41001, Some(1_792_826_400), "claude.exe");
        assert_eq!(check_liveness(Some(&alive), &expected), Liveness::Alive);
        let reused = info(41001, Some(1_000), "notepad.exe");
        assert_eq!(check_liveness(Some(&reused), &expected), Liveness::Reused);
        let no_time = ProcessRef::pid_only(41001);
        assert_eq!(
            check_liveness(Some(&alive), &no_time),
            Liveness::AliveUnverified
        );
        assert_eq!(check_liveness(Some(&reused), &no_time), Liveness::Reused);
        let node = info(41001, None, "node");
        assert_eq!(
            check_liveness(Some(&node), &expected),
            Liveness::AliveUnverified
        );
        assert!(Liveness::AliveUnverified.is_alive());
        assert!(!Liveness::Reused.is_alive());
    }

    #[test]
    fn claude_names() {
        assert!(looks_like_claude(&info(1, None, "claude.exe")));
        assert!(looks_like_claude(&info(1, None, "Node")));
        assert!(!looks_like_claude(&info(1, None, "pwsh.exe")));
        let by_exe = ProcessInfo {
            exe: Some(PathBuf::from("/usr/local/bin/claude")),
            ..ProcessInfo::default()
        };
        assert!(looks_like_claude(&by_exe));
    }

    #[test]
    fn fake_probe_round_trip() {
        let mut probe = FakeProbe::new();
        probe.insert(info(7, Some(1), "claude"));
        assert_eq!(probe.probe(7).unwrap().pid, 7);
        probe.remove(7);
        assert!(probe.probe(7).is_none());
    }

    #[test]
    fn sysinfo_probe_sees_this_process_and_keeps_only_host_keys() {
        let mut probe = SysinfoProbe::new();
        let me = probe
            .probe(std::process::id())
            .expect("the test process exists");
        assert_eq!(me.pid, std::process::id());
        assert!(me.start_time_secs.is_some());
        assert!(me.env.iter().all(|(k, _)| keeps_env_key(k)), "{:?}", me.env);
        assert!(probe.probe(u32::MAX - 1).is_none());
    }
}
