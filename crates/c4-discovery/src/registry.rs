// <wtl> project: c4 | file: crates/c4-discovery/src/registry.rs | author: watchthelight </wtl>
//! The session registry: `~/.claude/sessions/<pid>.json`, one file per running Claude Code process,
//! written by Claude Code itself and removed when the process exits. Next to each record sits a
//! `<pid>.<hash>.key` file that holds the peer-message secret; the scanner never opens it.

use crate::Problem;
use c4_core::{
    Observation, ObservationBody, ProcessRef, RegistryStatus, SessionId, SessionKind, Timestamp,
};
use serde::{Deserialize, Deserializer};
use std::path::{Path, PathBuf};

/// One registry record. Every field but `pid` and `sessionId` is optional, because older versions
/// wrote fewer fields and newer ones may add more; unknown fields are ignored.
#[derive(Clone, PartialEq, Eq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    /// The process id.
    pub pid: u32,
    /// The session id.
    pub session_id: String,
    /// The working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// When the process started, milliseconds since the epoch.
    #[serde(default)]
    pub started_at: Option<i64>,
    /// The process creation time in the platform's encoding (a decimal string since 2.1.29x, a number
    /// before that). On Windows it is the creation `FILETIME`.
    #[serde(default, deserialize_with = "proc_start")]
    pub proc_start: Option<u64>,
    /// The Claude Code version.
    #[serde(default)]
    pub version: Option<String>,
    /// `interactive` or `background`.
    #[serde(default)]
    pub kind: Option<String>,
    /// `cli` or another entry point.
    #[serde(default)]
    pub entrypoint: Option<String>,
    /// The pid domain, such as `win32:<hex>`.
    #[serde(default)]
    pub pid_domain: Option<String>,
    /// The peer protocol version.
    #[serde(default)]
    pub peer_protocol: Option<u32>,
    /// The peer-message socket or pipe path.
    #[serde(default)]
    pub messaging_socket_path: Option<String>,
    /// The display name.
    #[serde(default)]
    pub name: Option<String>,
    /// `user`, `auto` or `derived`.
    #[serde(default)]
    pub name_source: Option<String>,
    /// When the record was last written.
    #[serde(default)]
    pub updated_at: Option<i64>,
    /// `busy`, `idle` or `waiting`.
    #[serde(default)]
    pub status: Option<RegistryStatus>,
    /// Why the session waits, when it does.
    #[serde(default)]
    pub waiting_for: Option<String>,
    /// When the status last changed.
    #[serde(default)]
    pub status_updated_at: Option<i64>,
}

/// `procStart` as a string or a number; anything else reads as unknown.
pub(crate) fn proc_start<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Num(u64),
        Str(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Num(n) => Some(n),
        Raw::Str(s) => s.trim().parse().ok(),
        Raw::Other(_) => None,
    })
}

impl RegistryEntry {
    /// The session id.
    #[must_use]
    pub fn session(&self) -> SessionId {
        SessionId::new(self.session_id.as_str())
    }

    /// The process reference, with the creation time when the record had one.
    #[must_use]
    pub fn process(&self) -> ProcessRef {
        match self.proc_start {
            Some(start) => ProcessRef::new(self.pid, start),
            None => ProcessRef::pid_only(self.pid),
        }
    }

    /// Background when the record says so, interactive otherwise.
    #[must_use]
    pub fn session_kind(&self) -> SessionKind {
        if self.kind.as_deref() == Some("background") {
            SessionKind::Background
        } else {
            SessionKind::Interactive
        }
    }

    /// The time the record speaks for: the status change time, else the write time, else `fallback`.
    #[must_use]
    pub fn observed_at(&self, fallback: Timestamp) -> Timestamp {
        self.status_updated_at
            .or(self.updated_at)
            .map_or(fallback, Timestamp::from_millis)
    }

    /// The observation this record stands for.
    #[must_use]
    pub fn observation(&self, fallback: Timestamp) -> Observation {
        Observation::new(
            self.session(),
            self.observed_at(fallback),
            ObservationBody::Registry {
                process: self.process(),
                session_kind: self.session_kind(),
                cwd: self.cwd.clone(),
                name: self.name.clone(),
                name_source: self.name_source.clone(),
                status: self.status.unwrap_or_default(),
                waiting_for: self.waiting_for.clone(),
                version: self.version.clone(),
                socket: self.messaging_socket_path.clone(),
                started_at: self.started_at.map(Timestamp::from_millis),
            },
        )
    }
}

/// What a scan of the registry directory found.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RegistryScan {
    /// The records that parsed, in pid order.
    pub entries: Vec<RegistryEntry>,
    /// Files that looked like records and did not parse.
    pub problems: Vec<Problem>,
    /// Files that were not opened: key files, notes, anything without a `.json` extension.
    pub skipped: Vec<PathBuf>,
}

/// True for the files the scanner opens: a `.json` extension, any case.
#[must_use]
pub fn is_registry_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
}

/// Read one record.
pub fn read_entry(path: &Path) -> Result<RegistryEntry, Problem> {
    let text = std::fs::read_to_string(path).map_err(|e| Problem::new(path, e.to_string()))?;
    let entry: RegistryEntry = serde_json::from_str(&text)
        .map_err(|e| Problem::new(path, format!("not a registry record: {e}")))?;
    Ok(entry)
}

/// Scan a registry directory. A missing directory is an empty registry, not an error.
#[must_use]
pub fn scan_registry(dir: &Path) -> RegistryScan {
    let mut scan = RegistryScan::default();
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return scan,
        Err(e) => {
            scan.problems.push(Problem::new(dir, e.to_string()));
            return scan;
        }
    };
    for item in read {
        let path = match item {
            Ok(item) => item.path(),
            Err(e) => {
                scan.problems.push(Problem::new(dir, e.to_string()));
                continue;
            }
        };
        if !path.is_file() || !is_registry_file(&path) {
            scan.skipped.push(path);
            continue;
        }
        match read_entry(&path) {
            Ok(entry) => {
                let stem_pid: Option<u32> = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse().ok());
                if let Some(stem) = stem_pid
                    && stem != entry.pid
                {
                    scan.problems.push(Problem::new(
                        &path,
                        format!(
                            "file is named for pid {stem} but the record says pid {}",
                            entry.pid
                        ),
                    ));
                }
                scan.entries.push(entry);
            }
            Err(problem) => scan.problems.push(problem),
        }
    }
    scan.entries.sort_by_key(|e| e.pid);
    scan.skipped.sort();
    scan.problems.sort_by(|a, b| a.path.cmp(&b.path));
    scan
}

#[cfg(test)]
mod tests {
    use super::*;
    use c4_core::StateKind;

    fn fixture_sessions() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/home/sessions")
    }

    #[test]
    fn scans_records_and_never_opens_key_files() {
        let scan = scan_registry(&fixture_sessions());
        let pids: Vec<u32> = scan.entries.iter().map(|e| e.pid).collect();
        assert_eq!(pids, [41001, 41002, 41003, 41004]);
        assert_eq!(scan.problems.len(), 1, "{:?}", scan.problems);
        assert!(scan.problems[0].path.ends_with("broken.json"));
        let skipped: Vec<String> = scan
            .skipped
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        let is_key = |name: &str| {
            Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("key"))
        };
        assert!(skipped.iter().any(|n| is_key(n)), "{skipped:?}");
        assert!(skipped.contains(&"notes.txt".to_string()));
        assert!(
            scan.problems
                .iter()
                .all(|p| !is_key(&p.path.to_string_lossy())),
            "a key file was opened"
        );
    }

    #[test]
    fn missing_directory_is_empty() {
        let scan = scan_registry(Path::new("/definitely/not/here/sessions"));
        assert_eq!(scan.entries, Vec::new());
        assert_eq!(scan.problems, Vec::new());
    }

    #[test]
    fn proc_start_accepts_string_and_number() {
        let scan = scan_registry(&fixture_sessions());
        let by_pid = |pid: u32| scan.entries.iter().find(|e| e.pid == pid).unwrap();
        assert_eq!(by_pid(41001).proc_start, Some(134_373_000_000_000_000));
        assert_eq!(by_pid(41004).proc_start, Some(134_373_004_000_000_000));
        assert_eq!(by_pid(41004).status, None);
        assert_eq!(by_pid(41002).status, Some(RegistryStatus::Waiting));
        assert_eq!(by_pid(41002).waiting_for.as_deref(), Some("approve Bash"));
        let odd: RegistryEntry =
            serde_json::from_str(r#"{"pid":1,"sessionId":"s","procStart":{"weird":true}}"#)
                .unwrap();
        assert_eq!(odd.proc_start, None);
        assert_eq!(odd.process(), ProcessRef::pid_only(1));
    }

    #[test]
    fn observation_carries_the_record_time_and_status() {
        let scan = scan_registry(&fixture_sessions());
        let waiting = scan.entries.iter().find(|e| e.pid == 41002).unwrap();
        let obs = waiting.observation(Timestamp::from_millis(0));
        assert_eq!(obs.at, Timestamp::from_millis(1_792_900_200_000));
        let mut view = c4_core::SessionView::new(obs.session.clone(), Timestamp::from_millis(0));
        let t = c4_core::reduce(&mut view, &obs).unwrap();
        assert_eq!(t.to.kind(), StateKind::WaitingPermission);
        assert_eq!(view.display_name(), "nightly build");
        assert_eq!(
            view.process,
            Some(ProcessRef::new(41002, 134_373_001_000_000_000))
        );
        let old = scan.entries.iter().find(|e| e.pid == 41004).unwrap();
        let obs = old.observation(Timestamp::from_millis(7));
        assert_eq!(obs.at, Timestamp::from_millis(1_792_900_400_000));
        let mut view = c4_core::SessionView::new(obs.session.clone(), Timestamp::from_millis(0));
        assert!(
            c4_core::reduce(&mut view, &obs).is_none(),
            "no status means no transition"
        );
        assert_eq!(view.state.kind(), StateKind::Idle);
    }
}
