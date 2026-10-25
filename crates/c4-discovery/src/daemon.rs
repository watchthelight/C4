// <wtl> project: c4 | file: crates/c4-discovery/src/daemon.rs | author: watchthelight </wtl>
//! The background session supervisor's files: `daemon/roster.json` (the workers the supervisor
//! runs) and `jobs/<short>/state.json` (what each job is doing). Both are Claude Code internals with
//! no documented format, so the parsers take what they recognize and ignore the rest. The roster
//! carries the rendezvous and PTY auth secrets; they are removed from the parsed JSON before any
//! struct exists, and nothing here keeps the raw text.

use crate::Problem;
use crate::home::ClaudeHome;
use crate::registry::proc_start;
use c4_core::{BgState, Observation, ObservationBody, ProcessRef, SessionId, Timestamp};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::path::Path;

/// Keys removed from supervisor JSON before parsing, whatever their depth, on top of every key whose
/// name contains `auth` or `credential` and every key [`c4_core::redact::is_secret_name`] rejects
/// (token, secret, password, cookie, api key, private key). The plain `tokens` counter in a job
/// state is the one exception.
pub const DENIED_KEYS: &[&str] = &["rvAuth", "ptyAuth", "env", "providerEnv", "headers"];

/// True for a key that is dropped.
#[must_use]
pub fn is_denied_key(key: &str) -> bool {
    if DENIED_KEYS.iter().any(|d| d.eq_ignore_ascii_case(key)) {
        return true;
    }
    let lower = key.to_ascii_lowercase();
    if lower == "tokens" {
        return false;
    }
    lower.contains("auth") || lower.contains("credential") || c4_core::redact::is_secret_name(key)
}

/// Remove denied keys at every depth.
pub fn strip_secrets(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|k, _| !is_denied_key(k));
            for v in map.values_mut() {
                strip_secrets(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_secrets),
        _ => {}
    }
}

/// How a worker was launched, from the roster's `dispatch.launch`.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    /// `prompt` or `resume`.
    #[serde(default)]
    pub mode: Option<String>,
    /// True when the resume forked the conversation.
    #[serde(default)]
    pub fork: bool,
    /// True when the launch restores a transcript.
    #[serde(default)]
    pub restores_transcript: bool,
}

/// The intent and name a job was seeded with.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seed {
    /// The first prompt or intent line.
    #[serde(default)]
    pub intent: Option<String>,
    /// The display name.
    #[serde(default)]
    pub name: Option<String>,
}

/// The dispatch record of a worker: what was asked for when the job was created.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dispatch {
    /// Where the request came from (`slash`, `cli`).
    #[serde(default)]
    pub source: Option<String>,
    /// How the worker was launched.
    #[serde(default)]
    pub launch: Option<Launch>,
    /// The seed.
    #[serde(default)]
    pub seed: Option<Seed>,
    /// Flags a respawn repeats.
    #[serde(default)]
    pub respawn_flags: Vec<String>,
}

/// One worker in the roster, with its secrets already gone.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RosterWorker {
    /// The short job id (the roster key).
    #[serde(skip)]
    pub short: String,
    /// The worker pid.
    #[serde(default)]
    pub pid: Option<u32>,
    /// The worker's creation time, in the same encoding as the registry's `procStart`.
    #[serde(default, deserialize_with = "proc_start")]
    pub proc_start: Option<u64>,
    /// The session id.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The Claude Code version of the worker.
    #[serde(default)]
    pub cli_version: Option<String>,
    /// When the worker started, milliseconds since the epoch.
    #[serde(default)]
    pub started_at: Option<i64>,
    /// How many times the supervisor has started this job.
    #[serde(default)]
    pub attempt: Option<u32>,
    /// The working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// The dispatch record.
    #[serde(default)]
    pub dispatch: Option<Dispatch>,
}

impl RosterWorker {
    /// The process reference, when the roster names a pid.
    #[must_use]
    pub fn process(&self) -> Option<ProcessRef> {
        let pid = self.pid?;
        Some(match self.proc_start {
            Some(start) => ProcessRef::new(pid, start),
            None => ProcessRef::pid_only(pid),
        })
    }
}

/// The roster: the supervisor and its workers.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Roster {
    /// The roster protocol version.
    pub proto: Option<u64>,
    /// The supervisor pid.
    pub supervisor_pid: Option<u32>,
    /// When the roster was last written, milliseconds since the epoch.
    pub updated_at: Option<i64>,
    /// The workers, in short id order.
    pub workers: Vec<RosterWorker>,
    /// Workers that did not parse.
    pub problems: Vec<Problem>,
}

/// Read the roster. Secrets are stripped from the parsed JSON before any worker is built.
pub fn read_roster(path: &Path) -> Result<Roster, Problem> {
    let text = std::fs::read_to_string(path).map_err(|e| Problem::new(path, e.to_string()))?;
    let mut value: Value = serde_json::from_str(&text)
        .map_err(|e| Problem::new(path, format!("not a roster: {e}")))?;
    drop(text);
    strip_secrets(&mut value);
    let mut roster = Roster {
        proto: value.get("proto").and_then(Value::as_u64),
        supervisor_pid: value
            .get("supervisorPid")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok()),
        updated_at: value.get("updatedAt").and_then(Value::as_i64),
        ..Roster::default()
    };
    if let Some(workers) = value.get("workers").and_then(Value::as_object) {
        for (short, raw) in workers {
            match serde_json::from_value::<RosterWorker>(raw.clone()) {
                Ok(mut worker) => {
                    worker.short.clone_from(short);
                    roster.workers.push(worker);
                }
                Err(e) => roster
                    .problems
                    .push(Problem::new(path, format!("worker {short}: {e}"))),
            }
        }
    }
    roster.workers.sort_by(|a, b| a.short.cmp(&b.short));
    Ok(roster)
}

/// The supervisor's count of work in flight for a job.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InFlight {
    /// Running tasks.
    #[serde(default)]
    pub tasks: u32,
    /// Queued tasks.
    #[serde(default)]
    pub queued: u32,
    /// The kinds of the running tasks.
    #[serde(default)]
    pub kinds: Vec<String>,
}

/// One running task of a job.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FanEntry {
    /// The task id.
    #[serde(default)]
    pub id: String,
    /// `shell`, `subagent`, and others.
    #[serde(default)]
    pub kind: String,
    /// The label.
    #[serde(default)]
    pub label: Option<String>,
    /// When it started, milliseconds since the epoch.
    #[serde(default)]
    pub started_at: Option<i64>,
}

/// A time as the job files write it: an RFC 3339 string, or epoch milliseconds or seconds.
fn flexible_time<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Timestamp>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw {
        Num(i64),
        Str(String),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Raw::deserialize(d)? {
        Raw::Num(n) if n > 100_000_000_000 => Some(Timestamp::from_millis(n)),
        Raw::Num(n) => Some(Timestamp::from_secs(n)),
        Raw::Str(s) => Timestamp::parse_rfc3339(&s)
            .or_else(|| s.trim().parse::<i64>().ok().map(Timestamp::from_millis)),
        Raw::Other(_) => None,
    })
}

/// A job's state file.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    /// The short job id (the directory name).
    #[serde(skip)]
    pub short: String,
    /// `working`, `blocked`, `done`, `failed` or `stopped`.
    #[serde(default)]
    pub state: BgState,
    /// The detail line.
    #[serde(default)]
    pub detail: Option<String>,
    /// `active`, `idle` or `blocked`.
    #[serde(default)]
    pub tempo: Option<String>,
    /// Work in flight.
    #[serde(default)]
    pub in_flight: InFlight,
    /// The running tasks.
    #[serde(default)]
    pub fan: Vec<FanEntry>,
    /// The supervisor's token counter.
    #[serde(default)]
    pub tokens: Option<u64>,
    /// What the job needs from the user; empty when nothing.
    #[serde(default)]
    pub needs: Option<String>,
    /// The transcript the supervisor scans for links.
    #[serde(default)]
    pub link_scan_path: Option<String>,
    /// The intent line.
    #[serde(default)]
    pub intent: Option<String>,
    /// The display name.
    #[serde(default)]
    pub name: Option<String>,
    /// `user`, `auto` or `derived`.
    #[serde(default)]
    pub name_source: Option<String>,
    /// The session id.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The short id as the file states it.
    #[serde(default)]
    pub daemon_short: Option<String>,
    /// The Claude Code version.
    #[serde(default)]
    pub cli_version: Option<String>,
    /// The working directory.
    #[serde(default)]
    pub cwd: Option<String>,
    /// `daemon` or another backend.
    #[serde(default)]
    pub backend: Option<String>,
    /// When the job was created.
    #[serde(default, deserialize_with = "flexible_time")]
    pub created_at: Option<Timestamp>,
    /// When the file was last written.
    #[serde(default, deserialize_with = "flexible_time")]
    pub updated_at: Option<Timestamp>,
}

impl JobState {
    /// How much work is in flight: the larger of the counter and the fan list.
    #[must_use]
    pub fn in_flight_count(&self) -> u32 {
        self.in_flight.tasks.max(self.fan.len() as u32)
    }
}

/// Read `state.json` from a job directory.
pub fn read_job_state(dir: &Path) -> Result<JobState, Problem> {
    let path = dir.join("state.json");
    let text = std::fs::read_to_string(&path).map_err(|e| Problem::new(&path, e.to_string()))?;
    let mut value: Value = serde_json::from_str(&text)
        .map_err(|e| Problem::new(&path, format!("not a job state: {e}")))?;
    drop(text);
    strip_secrets(&mut value);
    let mut job: JobState = serde_json::from_value(value)
        .map_err(|e| Problem::new(&path, format!("not a job state: {e}")))?;
    job.short = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(job)
}

/// Read every job directory under `jobs/`. Files at the top level (`pins.json`) are ignored, and so
/// are directories without a `state.json`.
#[must_use]
pub fn scan_jobs(jobs_dir: &Path) -> (Vec<JobState>, Vec<Problem>) {
    let mut jobs = Vec::new();
    let mut problems = Vec::new();
    let read = match std::fs::read_dir(jobs_dir) {
        Ok(read) => read,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (jobs, problems),
        Err(e) => {
            problems.push(Problem::new(jobs_dir, e.to_string()));
            return (jobs, problems);
        }
    };
    for item in read.flatten() {
        let dir = item.path();
        if !dir.is_dir() || !dir.join("state.json").is_file() {
            continue;
        }
        match read_job_state(&dir) {
            Ok(job) => jobs.push(job),
            Err(problem) => problems.push(problem),
        }
    }
    jobs.sort_by(|a, b| a.short.cmp(&b.short));
    (jobs, problems)
}

/// What a scan of the supervisor files found.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DaemonScan {
    /// The roster, when there is one.
    pub roster: Option<Roster>,
    /// The jobs, in short id order.
    pub jobs: Vec<JobState>,
    /// Files that did not parse.
    pub problems: Vec<Problem>,
}

/// Scan the roster and the jobs. Missing files mean no background sessions, not an error.
#[must_use]
pub fn scan_daemon(home: &ClaudeHome) -> DaemonScan {
    let mut scan = DaemonScan::default();
    let roster_path = home.roster_path();
    if roster_path.is_file() {
        match read_roster(&roster_path) {
            Ok(mut roster) => {
                scan.problems.append(&mut roster.problems);
                scan.roster = Some(roster);
            }
            Err(problem) => scan.problems.push(problem),
        }
    }
    let (jobs, mut problems) = scan_jobs(&home.jobs_dir());
    scan.jobs = jobs;
    scan.problems.append(&mut problems);
    scan
}

/// A job and its worker, joined on the short id. Either side can be missing: a job directory
/// outlives its worker, and a roster can name a worker whose job directory is gone.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BackgroundJob {
    /// The short job id.
    pub short: String,
    /// The job state, when the directory exists.
    pub job: Option<JobState>,
    /// The roster worker, when the roster lists one.
    pub worker: Option<RosterWorker>,
}

impl BackgroundJob {
    /// The session id, from the job state or the roster.
    #[must_use]
    pub fn session(&self) -> Option<SessionId> {
        self.job
            .as_ref()
            .and_then(|j| j.session_id.as_deref())
            .or_else(|| self.worker.as_ref().and_then(|w| w.session_id.as_deref()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(SessionId::new)
    }

    /// The worker process, when the roster names one.
    #[must_use]
    pub fn process(&self) -> Option<ProcessRef> {
        self.worker.as_ref().and_then(RosterWorker::process)
    }

    /// The observation this job stands for, dated by the state file's own write time.
    #[must_use]
    pub fn observation(&self, now: Timestamp) -> Option<Observation> {
        let session = self.session()?;
        let job = self.job.as_ref();
        let worker = self.worker.as_ref();
        let seed = worker
            .and_then(|w| w.dispatch.as_ref())
            .and_then(|d| d.seed.as_ref());
        let pick = |a: Option<&String>, b: Option<&String>| a.or(b).cloned();
        Some(Observation::new(
            session,
            job.and_then(|j| j.updated_at).unwrap_or(now),
            ObservationBody::Daemon {
                short: self.short.clone(),
                process: self.process(),
                state: job.map_or(BgState::Unknown, |j| j.state),
                tempo: job.and_then(|j| j.tempo.clone()),
                detail: job.and_then(|j| j.detail.clone()),
                needs: job.and_then(|j| j.needs.clone()),
                in_flight: job.map_or(0, JobState::in_flight_count),
                cwd: pick(
                    job.and_then(|j| j.cwd.as_ref()),
                    worker.and_then(|w| w.cwd.as_ref()),
                ),
                name: pick(
                    job.and_then(|j| j.name.as_ref()),
                    seed.and_then(|s| s.name.as_ref()),
                ),
                name_source: job.and_then(|j| j.name_source.clone()),
                version: pick(
                    job.and_then(|j| j.cli_version.as_ref()),
                    worker.and_then(|w| w.cli_version.as_ref()),
                ),
            },
        ))
    }
}

impl DaemonScan {
    /// Jobs and workers joined on the short id, in short id order.
    #[must_use]
    pub fn jobs_joined(&self) -> Vec<BackgroundJob> {
        let mut by_short: std::collections::BTreeMap<String, BackgroundJob> =
            std::collections::BTreeMap::new();
        for job in &self.jobs {
            by_short.insert(
                job.short.clone(),
                BackgroundJob {
                    short: job.short.clone(),
                    job: Some(job.clone()),
                    worker: None,
                },
            );
        }
        if let Some(roster) = &self.roster {
            for worker in &roster.workers {
                by_short
                    .entry(worker.short.clone())
                    .or_insert_with(|| BackgroundJob {
                        short: worker.short.clone(),
                        job: None,
                        worker: None,
                    })
                    .worker = Some(worker.clone());
            }
        }
        by_short.into_values().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    fn fixture_home() -> ClaudeHome {
        ClaudeHome::at(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/home"))
    }

    #[test]
    fn strip_secrets_recurses_and_keeps_the_token_counter() {
        let mut v = json!({
            "rvAuth": "a", "token": "b", "tokens": 5, "apiKey": "c",
            "nested": [{"ptyAuth": "d", "ok": 1}, {"env": {"X": "y"}, "cwd": "/tmp"}]
        });
        strip_secrets(&mut v);
        assert_eq!(
            v,
            json!({"tokens": 5, "nested": [{"ok": 1}, {"cwd": "/tmp"}]})
        );
        assert!(is_denied_key("rvAuth"));
        assert!(is_denied_key("providerEnv"));
        assert!(is_denied_key("accessToken"));
        assert!(!is_denied_key("tokens"));
        assert!(!is_denied_key("sessionId"));
    }

    #[test]
    fn roster_drops_auth_before_anything_is_kept() {
        let roster = read_roster(&fixture_home().roster_path()).unwrap();
        assert_eq!(roster.supervisor_pid, Some(41900));
        assert_eq!(roster.workers.len(), 2);
        assert_eq!(roster.problems, Vec::new());
        let w0 = &roster.workers[0];
        assert_eq!(w0.short, "0b2c3d4e");
        assert_eq!(
            w0.process(),
            Some(ProcessRef::new(41011, 134_373_011_000_000_000))
        );
        assert_eq!(
            w0.session_id.as_deref(),
            Some("0b2c3d4e-0002-4000-8000-000000000011")
        );
        let launch = w0.dispatch.as_ref().unwrap().launch.as_ref().unwrap();
        assert_eq!(launch.mode.as_deref(), Some("prompt"));
        assert!(!launch.fork);
        let w1 = &roster.workers[1];
        let launch = w1.dispatch.as_ref().unwrap().launch.as_ref().unwrap();
        assert_eq!(launch.mode.as_deref(), Some("resume"));
        assert!(launch.fork);
        let dump = format!("{roster:?}");
        assert!(
            !dump.contains("fixture-not-a-secret"),
            "auth value survived"
        );
        assert!(!dump.contains("rvAuth") && !dump.contains("ptyAuth"));
    }

    #[test]
    fn job_states_parse_with_times() {
        let (jobs, problems) = scan_jobs(&fixture_home().jobs_dir());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(jobs.len(), 2, "pins.json is not a job");
        let j0 = &jobs[0];
        assert_eq!(j0.short, "0b2c3d4e");
        assert_eq!(j0.state, BgState::Working);
        assert_eq!(j0.tempo.as_deref(), Some("active"));
        assert_eq!(j0.in_flight_count(), 1);
        assert_eq!(j0.fan[0].label.as_deref(), Some("cargo test --workspace"));
        assert_eq!(j0.tokens, Some(12_345));
        assert_eq!(
            j0.updated_at,
            Timestamp::parse_rfc3339("2026-10-25T10:05:00.000Z")
        );
        let j1 = &jobs[1];
        assert_eq!(j1.state, BgState::Blocked);
        assert_eq!(
            j1.needs.as_deref(),
            Some("Approve the Bash command or stop the session")
        );
        let numeric: JobState =
            serde_json::from_str(r#"{"state":"done","updatedAt":1792900000000}"#).unwrap();
        assert_eq!(
            numeric.updated_at,
            Some(Timestamp::from_millis(1_792_900_000_000))
        );
        let secs: JobState = serde_json::from_str(r#"{"updatedAt":1792900000}"#).unwrap();
        assert_eq!(secs.updated_at, Some(Timestamp::from_secs(1_792_900_000)));
        assert_eq!(secs.state, BgState::Unknown);
    }

    #[test]
    fn joined_jobs_yield_daemon_observations() {
        let scan = scan_daemon(&fixture_home());
        assert!(scan.problems.is_empty(), "{:?}", scan.problems);
        let joined = scan.jobs_joined();
        assert_eq!(joined.len(), 2);
        let first = &joined[0];
        assert!(first.job.is_some() && first.worker.is_some());
        let obs = first.observation(Timestamp::from_millis(0)).unwrap();
        assert_eq!(obs.session.as_str(), "0b2c3d4e-0002-4000-8000-000000000011");
        assert_eq!(
            obs.at,
            Timestamp::parse_rfc3339("2026-10-25T10:05:00Z").unwrap()
        );
        match obs.body {
            ObservationBody::Daemon {
                short,
                process,
                state,
                in_flight,
                name,
                version,
                ..
            } => {
                assert_eq!(short, "0b2c3d4e");
                assert_eq!(
                    process,
                    Some(ProcessRef::new(41011, 134_373_011_000_000_000))
                );
                assert_eq!(state, BgState::Working);
                assert_eq!(in_flight, 1);
                assert_eq!(name.as_deref(), Some("worker one"));
                assert_eq!(version.as_deref(), Some("2.1.299"));
            }
            other => panic!("not a daemon observation: {other:?}"),
        }
        let orphan = BackgroundJob {
            short: "ffffffff".into(),
            job: None,
            worker: Some(RosterWorker {
                short: "ffffffff".into(),
                session_id: Some("ffffffff-0002-4000-8000-000000000099".into()),
                ..RosterWorker::default()
            }),
        };
        let obs = orphan.observation(Timestamp::from_millis(9)).unwrap();
        assert_eq!(obs.at, Timestamp::from_millis(9));
        assert!(matches!(
            obs.body,
            ObservationBody::Daemon {
                state: BgState::Unknown,
                process: None,
                ..
            }
        ));
        assert!(
            BackgroundJob {
                short: "x".into(),
                job: None,
                worker: None
            }
            .observation(Timestamp::from_millis(0))
            .is_none()
        );
    }

    #[test]
    fn missing_daemon_files_are_fine() {
        let tmp = tempfile::tempdir().unwrap();
        let scan = scan_daemon(&ClaudeHome::at(tmp.path()));
        assert!(scan.roster.is_none());
        assert_eq!(scan.jobs, Vec::new());
        assert_eq!(scan.problems, Vec::new());
    }
}
