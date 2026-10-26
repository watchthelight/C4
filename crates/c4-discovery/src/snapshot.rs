// <wtl> project: c4 | file: crates/c4-discovery/src/snapshot.rs | author: watchthelight </wtl>
//! One pass over every reader, and the fold of its observations into session views.

use crate::Problem;
use crate::agents::AgentsCommand;
use crate::daemon::scan_daemon;
use crate::home::ClaudeHome;
use crate::host::identify_host;
use crate::process::{Liveness, ProcessProbe, check_liveness};
use crate::registry::scan_registry;
use c4_core::{
    Attachment, DisconnectReason, Observation, ObservationBody, ProcessRef, SessionId, SessionKind,
    SessionTable, SessionView, Timestamp, Transition,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

/// What a snapshot reads and how.
#[derive(Clone, Debug)]
pub struct DiscoveryOptions {
    /// The Claude home.
    pub home: ClaudeHome,
    /// The session list command, or `None` to read files only.
    pub agents: Option<AgentsCommand>,
    /// How long the session list command may take.
    pub agents_timeout: Duration,
}

impl DiscoveryOptions {
    /// The home and the command from the environment, with a five second budget for the command.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            home: ClaudeHome::resolve(),
            agents: Some(AgentsCommand::from_env()),
            agents_timeout: Duration::from_secs(5),
        }
    }

    /// Read the files under `home` and run nothing.
    #[must_use]
    pub fn files_only(home: ClaudeHome) -> Self {
        Self {
            home,
            agents: None,
            agents_timeout: Duration::from_secs(5),
        }
    }

    /// Also run a session list command.
    #[must_use]
    pub fn with_agents(mut self, command: AgentsCommand) -> Self {
        self.agents = Some(command);
        self
    }
}

/// A process a session was last known to have. Passed back into a snapshot so that a session whose
/// registry entry vanished is still probed until its process is gone.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct KnownProcess {
    /// The session.
    pub session: SessionId,
    /// The process.
    pub process: ProcessRef,
    /// Interactive or background.
    pub kind: SessionKind,
}

/// Counts and problems from one snapshot.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize)]
pub struct SnapshotReport {
    /// Registry records read.
    pub registry_entries: usize,
    /// Registry files not opened (key files and the like).
    pub registry_skipped: usize,
    /// Entries the session list command returned, when it ran and succeeded.
    pub agents_entries: Option<usize>,
    /// Why the session list command gave nothing, when it ran and failed.
    pub agents_error: Option<String>,
    /// Workers in the supervisor roster.
    pub roster_workers: usize,
    /// Job directories read.
    pub jobs: usize,
    /// Sessions whose main transcript exists where their working directory says it should.
    pub transcripts: usize,
    /// Pids looked up in the process table.
    pub probed: usize,
    /// Pids found alive.
    pub alive: usize,
    /// Files that could not be used.
    pub problems: Vec<Problem>,
    /// How long the snapshot took.
    pub elapsed_ms: u64,
}

/// The observations of one pass.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// When the pass ran.
    pub at: Timestamp,
    /// Every observation, in the order to apply them.
    pub observations: Vec<Observation>,
    /// The sessions the registry listed.
    pub registry_sessions: BTreeSet<SessionId>,
    /// Counts and problems.
    pub report: SnapshotReport,
}

/// Run every reader once. `known` lists processes from earlier passes that must be checked even when
/// no file names them any more.
pub fn take_snapshot(
    opts: &DiscoveryOptions,
    probe: &mut dyn ProcessProbe,
    now: Timestamp,
    known: &[KnownProcess],
) -> Snapshot {
    let started = Instant::now();
    let mut report = SnapshotReport::default();
    let mut observations = Vec::new();
    let mut targets: BTreeMap<u32, KnownProcess> = BTreeMap::new();
    let mut orphans: Vec<SessionId> = Vec::new();
    let mut registry_sessions = BTreeSet::new();

    let registry = scan_registry(&opts.home.sessions_dir());
    report.registry_entries = registry.entries.len();
    report.registry_skipped = registry.skipped.len();
    report.problems.extend(registry.problems);
    for entry in &registry.entries {
        let session = entry.session();
        registry_sessions.insert(session.clone());
        observations.push(entry.observation(now));
        targets.entry(entry.pid).or_insert(KnownProcess {
            session,
            process: entry.process(),
            kind: entry.session_kind(),
        });
    }

    if let Some(command) = &opts.agents {
        match command.run(opts.agents_timeout) {
            Ok(entries) => {
                report.agents_entries = Some(entries.len());
                for entry in &entries {
                    let Some(obs) = entry.observation(now) else {
                        continue;
                    };
                    if let Some(pid) = entry.pid {
                        targets.entry(pid).or_insert(KnownProcess {
                            session: obs.session.clone(),
                            process: ProcessRef::pid_only(pid),
                            kind: entry.session_kind(),
                        });
                    }
                    observations.push(obs);
                }
            }
            Err(e) => report.agents_error = Some(e.to_string()),
        }
    }

    let daemon = scan_daemon(&opts.home);
    report.roster_workers = daemon.roster.as_ref().map_or(0, |r| r.workers.len());
    report.jobs = daemon.jobs.len();
    report.problems.extend(daemon.problems.iter().cloned());
    for job in daemon.jobs_joined() {
        let Some(obs) = job.observation(now) else {
            continue;
        };
        match job.process() {
            Some(process) => {
                targets.entry(process.pid).or_insert(KnownProcess {
                    session: obs.session.clone(),
                    process,
                    kind: SessionKind::Background,
                });
            }
            None => orphans.push(obs.session.clone()),
        }
        observations.push(obs);
    }

    // A session's main transcript sits at a path derived from its working directory. The file is
    // only checked for, never read here: the view carries the path from the first refresh so the
    // transcript crate knows what to follow.
    let mut cwds: BTreeMap<SessionId, String> = BTreeMap::new();
    for obs in &observations {
        let cwd = match &obs.body {
            ObservationBody::Registry { cwd, .. }
            | ObservationBody::Agents { cwd, .. }
            | ObservationBody::Daemon { cwd, .. } => cwd.as_deref(),
            _ => None,
        };
        if let Some(cwd) = cwd.filter(|c| !c.is_empty()) {
            cwds.entry(obs.session.clone())
                .or_insert_with(|| cwd.to_string());
        }
    }
    for (session, cwd) in cwds {
        let path = opts.home.transcript_path(&cwd, &session);
        if path.is_file() {
            report.transcripts += 1;
            observations.push(Observation::new(
                session,
                now,
                ObservationBody::Identity {
                    cwd: None,
                    name: None,
                    transcript_path: Some(path.display().to_string()),
                    version: None,
                },
            ));
        }
    }

    for k in known {
        targets.entry(k.process.pid).or_insert_with(|| k.clone());
    }

    for (pid, target) in &targets {
        report.probed += 1;
        let info = probe.probe(*pid);
        let verdict = check_liveness(info.as_ref(), &target.process);
        tracing::trace!(pid, session = %target.session, ?verdict, "probed");
        match verdict {
            Liveness::Alive | Liveness::AliveUnverified => {
                report.alive += 1;
                observations.push(Observation::new(
                    target.session.clone(),
                    now,
                    ObservationBody::ProcessAlive {
                        process: target.process,
                    },
                ));
                if target.kind == SessionKind::Interactive
                    && let Some(info) = &info
                {
                    let hint = identify_host(&info.env);
                    observations.push(Observation::new(
                        target.session.clone(),
                        now,
                        ObservationBody::Linked {
                            host: hint.host,
                            attachment: Attachment::Observed,
                            pane: hint.pane,
                        },
                    ));
                }
            }
            Liveness::Gone => observations.push(dead(target, DisconnectReason::ProcessGone, now)),
            Liveness::Reused => observations.push(dead(target, DisconnectReason::PidReused, now)),
        }
    }
    for session in orphans {
        observations.push(Observation::new(
            session,
            now,
            ObservationBody::ProcessDead {
                process: None,
                reason: DisconnectReason::ProcessGone,
            },
        ));
    }

    report.elapsed_ms = started.elapsed().as_millis() as u64;
    Snapshot {
        at: now,
        observations,
        registry_sessions,
        report,
    }
}

fn dead(target: &KnownProcess, reason: DisconnectReason, now: Timestamp) -> Observation {
    Observation::new(
        target.session.clone(),
        now,
        ObservationBody::ProcessDead {
            process: Some(target.process),
            reason,
        },
    )
}

/// What one refresh changed.
#[derive(Clone, Debug, Default)]
pub struct RefreshResult {
    /// State changes, in order.
    pub transitions: Vec<(SessionId, Transition)>,
    /// Every session an observation or the clock touched.
    pub touched: BTreeSet<SessionId>,
    /// The snapshot's counts and problems.
    pub report: SnapshotReport,
}

/// The session table kept current by repeated snapshots.
#[derive(Debug)]
pub struct Discovery {
    opts: DiscoveryOptions,
    table: SessionTable,
    registry_seen: BTreeSet<SessionId>,
}

impl Discovery {
    /// An empty table with these options.
    #[must_use]
    pub fn new(opts: DiscoveryOptions) -> Self {
        Self {
            opts,
            table: SessionTable::new(),
            registry_seen: BTreeSet::new(),
        }
    }

    /// The options.
    #[must_use]
    pub fn options(&self) -> &DiscoveryOptions {
        &self.opts
    }

    /// The table.
    #[must_use]
    pub fn table(&self) -> &SessionTable {
        &self.table
    }

    /// The table, for other readers to fold their observations into.
    pub fn table_mut(&mut self) -> &mut SessionTable {
        &mut self.table
    }

    /// Every session in attention order.
    #[must_use]
    pub fn views(&self) -> Vec<&SessionView> {
        self.table.sorted()
    }

    /// Take a snapshot and fold it in. Sessions that left the registry are told so; processes seen
    /// alive before are probed until they are gone.
    pub fn refresh(&mut self, probe: &mut dyn ProcessProbe, now: Timestamp) -> RefreshResult {
        let known: Vec<KnownProcess> = self
            .table
            .iter()
            .filter(|v| v.alive && !v.ended)
            .filter_map(|v| {
                v.process.map(|process| KnownProcess {
                    session: v.id.clone(),
                    process,
                    kind: v.session_kind,
                })
            })
            .collect();
        let snapshot = take_snapshot(&self.opts, probe, now, &known);
        let mut result = RefreshResult {
            report: snapshot.report,
            ..RefreshResult::default()
        };
        for obs in &snapshot.observations {
            if let Some(t) = self.table.apply(obs, now) {
                result.transitions.push((obs.session.clone(), t));
            }
            result.touched.insert(obs.session.clone());
        }
        for gone in self.registry_seen.difference(&snapshot.registry_sessions) {
            let obs = Observation::new(gone.clone(), now, ObservationBody::RegistryGone);
            if let Some(t) = self.table.apply(&obs, now) {
                result.transitions.push((gone.clone(), t));
            }
            result.touched.insert(gone.clone());
        }
        self.registry_seen = snapshot.registry_sessions;
        result.touched.extend(self.table.tick(now));
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{FakeProbe, ProcessInfo};
    use c4_core::{HostKind, SessionState, StateKind};
    use std::path::{Path, PathBuf};

    /// The fixtures root with native separators and no `..`, since cmd.exe reads `/` as a switch.
    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the workspace root")
            .join("fixtures")
    }

    fn agents_command() -> AgentsCommand {
        let file = fixtures()
            .join("agents-json")
            .join("active.json")
            .to_string_lossy()
            .into_owned();
        if cfg!(windows) {
            AgentsCommand::new("cmd.exe", ["/d", "/c", "type", &file])
        } else {
            AgentsCommand::new("cat", [file])
        }
    }

    fn proc(pid: u32, start: u64, name: &str, env: &[(&str, &str)]) -> ProcessInfo {
        ProcessInfo {
            pid,
            start_time_secs: Some(start),
            name: Some(name.into()),
            env: env
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            ..ProcessInfo::default()
        }
    }

    /// 41001 and 41004 alive, 41003 reused, 41011 (worker one) alive; 41002, 41012 and 41099 gone.
    fn probe() -> FakeProbe {
        let mut p = FakeProbe::new();
        p.insert(proc(
            41001,
            1_792_826_400,
            "claude.exe",
            &[("WEZTERM_PANE", "7")],
        ));
        p.insert(proc(41003, 1_000, "notepad.exe", &[]));
        p.insert(proc(
            41004,
            1_792_826_801,
            "node",
            &[("WT_SESSION", "0000-0000")],
        ));
        p.insert(proc(41011, 1_792_827_500, "claude", &[]));
        p
    }

    #[test]
    fn folds_the_fixture_home() {
        let opts = DiscoveryOptions::files_only(ClaudeHome::at(fixtures().join("home")))
            .with_agents(agents_command());
        let mut disc = Discovery::new(opts);
        let now = Timestamp::from_millis(1_792_900_900_000);
        let res = disc.refresh(&mut probe(), now);
        assert_eq!(res.report.registry_entries, 4);
        assert_eq!(res.report.agents_entries, Some(5));
        assert_eq!(res.report.roster_workers, 2);
        assert_eq!(res.report.jobs, 2);
        assert_eq!(
            res.report.probed, 7,
            "4 registry pids, 2 workers, 1 agents-only pid"
        );
        assert_eq!(res.report.alive, 3);
        assert_eq!(
            res.report.transcripts, 4,
            "alpha to delta have transcripts; the workers and the odd entry do not"
        );
        assert_eq!(res.report.problems.len(), 1, "{:?}", res.report.problems);
        assert_eq!(disc.table().len(), 7);

        let view = |id: &str| disc.table().get(&SessionId::new(id)).unwrap();
        let alpha = view("0a1b2c3d-0001-4000-8000-000000000001");
        assert_eq!(alpha.state, SessionState::Working);
        assert!(alpha.alive);
        assert_eq!(alpha.host, HostKind::WezTerm);
        assert_eq!(alpha.pane.as_deref(), Some("7"));
        assert_eq!(alpha.attachment, Attachment::Observed);
        assert_eq!(alpha.display_name(), "proj-alpha");
        assert_eq!(
            alpha.transcript_path.as_deref(),
            Some(
                fixtures()
                    .join("home")
                    .join("projects")
                    .join("-home-example-proj-alpha")
                    .join("0a1b2c3d-0001-4000-8000-000000000001.jsonl")
                    .display()
                    .to_string()
                    .as_str()
            ),
            "the transcript path is derived from the cwd and checked for"
        );

        let beta = view("0a1b2c3d-0001-4000-8000-000000000002");
        assert_eq!(
            beta.state,
            SessionState::Disconnected {
                reason: DisconnectReason::ProcessGone
            }
        );
        assert!(!beta.alive);
        assert_eq!(beta.attachment, Attachment::Disconnected);
        assert_eq!(beta.display_name(), "nightly build");

        let gamma = view("0a1b2c3d-0001-4000-8000-000000000003");
        assert_eq!(
            gamma.state,
            SessionState::Disconnected {
                reason: DisconnectReason::PidReused
            }
        );

        let delta = view("0a1b2c3d-0001-4000-8000-000000000004");
        assert_eq!(delta.state, SessionState::Idle);
        assert!(delta.alive);
        assert_eq!(delta.host, HostKind::WindowsTerminal);

        let worker_one = view("0b2c3d4e-0002-4000-8000-000000000011");
        assert_eq!(worker_one.state, SessionState::Working);
        assert!(worker_one.alive);
        assert_eq!(worker_one.session_kind, SessionKind::Background);
        assert_eq!(worker_one.daemon_short.as_deref(), Some("0b2c3d4e"));
        assert_eq!(worker_one.daemon_in_flight, 1);
        assert_eq!(worker_one.display_name(), "worker one");
        assert_eq!(
            worker_one.host,
            HostKind::Unknown,
            "background workers have no terminal host"
        );

        let worker_two = view("0c3d4e5f-0002-4000-8000-000000000012");
        assert_eq!(
            worker_two.state.kind(),
            StateKind::Disconnected,
            "a blocked job with a dead worker"
        );
        assert!(!worker_two.alive);
        assert_eq!(worker_two.daemon_short.as_deref(), Some("0c3d4e5f"));

        let odd = view("0a1b2c3d-0001-4000-8000-000000000099");
        assert_eq!(odd.state.kind(), StateKind::Disconnected);
        assert_eq!(odd.transcript_path, None, "no file under its project dir");

        let order: Vec<&str> = disc.views().iter().map(|v| v.id.as_str()).collect();
        assert_eq!(order.len(), 7);
        assert!(res.transitions.len() >= 6, "{:?}", res.transitions);
    }

    #[test]
    fn registry_gone_keeps_a_live_session_until_its_process_dies() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let record = sessions.join("41001.json");
        let source = fixtures().join("home").join("sessions").join("41001.json");
        std::fs::copy(source, &record).unwrap();
        let mut d = Discovery::new(DiscoveryOptions::files_only(ClaudeHome::at(tmp.path())));
        let mut p = probe();
        let sid = SessionId::new("0a1b2c3d-0001-4000-8000-000000000001");
        let t0 = Timestamp::from_millis(1_792_900_900_000);

        d.refresh(&mut p, t0);
        assert_eq!(d.table().get(&sid).unwrap().state, SessionState::Working);

        std::fs::remove_file(&record).unwrap();
        let r = d.refresh(&mut p, t0.plus_millis(5_000));
        assert_eq!(r.report.registry_entries, 0);
        assert_eq!(r.report.probed, 1, "the process is still checked");
        let v = d.table().get(&sid).unwrap();
        assert_eq!(
            v.state,
            SessionState::Working,
            "alive without a registry entry"
        );
        assert!(v.alive);

        p.remove(41001);
        let r = d.refresh(&mut p, t0.plus_millis(10_000));
        assert_eq!(r.report.probed, 1);
        let v = d.table().get(&sid).unwrap();
        assert_eq!(
            v.state,
            SessionState::Disconnected {
                reason: DisconnectReason::ProcessGone
            }
        );
        assert!(!v.alive);

        let r = d.refresh(&mut p, t0.plus_millis(15_000));
        assert_eq!(r.report.probed, 0, "a dead process is not probed again");
    }

    #[test]
    fn empty_home_gives_an_empty_table() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = Discovery::new(DiscoveryOptions::files_only(ClaudeHome::at(tmp.path())));
        let r = d.refresh(&mut FakeProbe::new(), Timestamp::from_millis(1));
        assert!(d.table().is_empty());
        assert_eq!(r.report.problems, Vec::new());
        assert_eq!(r.report.agents_entries, None);
    }

    #[test]
    fn a_failing_list_command_is_reported_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let opts = DiscoveryOptions::files_only(ClaudeHome::at(tmp.path())).with_agents(
            AgentsCommand::new("c4-no-such-program-0000", Vec::<String>::new()),
        );
        let mut d = Discovery::new(opts);
        let r = d.refresh(&mut FakeProbe::new(), Timestamp::from_millis(1));
        assert!(r.report.agents_error.is_some());
        assert_eq!(r.report.agents_entries, None);
    }
}
