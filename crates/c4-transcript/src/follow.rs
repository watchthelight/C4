// <wtl> project: c4 | file: crates/c4-transcript/src/follow.rs | author: watchthelight </wtl>
//! One session's transcript and the agent files beside it, turned into observations.

use crate::events::events_for;
use crate::record::{ModelUsage, Record, SUPPORTED_MAJOR, Usage, version_supported};
use crate::scan::{AgentFile, AgentMeta, scan_session_dir};
use crate::tailer::{Cursor, TailError, Tailer};
use c4_core::{Observation, ObservationBody, SessionId, Timestamp, TranscriptEvent};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How many polls look for a missing `.meta.json` before giving up on it.
const META_ATTEMPTS: u8 = 5;

/// What the transcript says about the session itself. Any field can change inside one file.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Identity {
    /// The working directory of the latest content record.
    pub cwd: Option<String>,
    /// The git branch of the latest content record.
    pub git_branch: Option<String>,
    /// The Claude Code version of the latest content record.
    pub version: Option<String>,
    /// The latest `ai-title`.
    pub ai_title: Option<String>,
    /// The latest `agent-name` (set with `--name` or `/rename`).
    pub agent_name: Option<String>,
    /// The latest `permission-mode`.
    pub permission_mode: Option<String>,
    /// The plan slug.
    pub slug: Option<String>,
    /// `bg` for background sessions.
    pub session_kind: Option<String>,
}

/// The latest `cost-state` record.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct CostState {
    /// Total cost in dollars.
    pub total_cost_usd: f64,
    /// Per-model totals.
    pub model_usage: BTreeMap<String, ModelUsage>,
}

/// What one agent file looks like right now.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AgentState {
    /// The file.
    pub file: AgentFile,
    /// Its meta file, once read.
    pub meta: Option<AgentMeta>,
    /// Tokens of the agent's own responses, max-merged by message id.
    pub usage: Usage,
    /// The earliest record time.
    pub first_activity: Option<Timestamp>,
    /// The latest record time.
    pub last_activity: Option<Timestamp>,
    /// The model of the latest assistant record.
    pub model: Option<String>,
}

/// What one poll of a session produced.
#[derive(Debug, Default)]
pub struct FollowPoll {
    /// Observations for the state machine, in order.
    pub observations: Vec<Observation>,
    /// Records read from the main transcript this time.
    pub records: Vec<Record>,
    /// Identity fields that changed.
    pub identity_changed: bool,
    /// Agents first seen this time.
    pub new_agents: Vec<String>,
    /// Problems worth a log line.
    pub notices: Vec<String>,
    /// True when the main transcript is gone.
    pub vanished: bool,
}

#[derive(Debug)]
struct AgentFollow {
    tailer: Tailer,
    state: AgentState,
    by_message: BTreeMap<String, Usage>,
    meta_attempts: u8,
    announced: bool,
}

impl AgentFollow {
    fn read_new(&mut self) {
        while let Ok(poll) = self.tailer.poll() {
            if poll.truncated {
                self.by_message.clear();
                self.state.first_activity = None;
            }
            for record in &poll.records {
                if let Some(at) = record.at() {
                    if self.state.first_activity.is_none() {
                        self.state.first_activity = Some(at);
                    }
                    self.state.last_activity = Some(at);
                }
                if record.kind == "assistant" {
                    merge_usage(&mut self.by_message, record);
                    if let Some(model) = record
                        .message
                        .as_ref()
                        .and_then(|m| m.model.as_deref())
                        .filter(|m| *m != "<synthetic>")
                    {
                        self.state.model = Some(model.to_string());
                    }
                }
            }
            if !poll.more {
                break;
            }
        }
        self.state.usage = sum_usage(self.by_message.values());
    }
}

/// Follows one session: its main transcript and the agent files under its directory.
#[derive(Debug)]
pub struct SessionFollower {
    session: SessionId,
    main: Tailer,
    identity: Identity,
    cost: Option<CostState>,
    usage_by_message: BTreeMap<String, Usage>,
    agents: BTreeMap<String, AgentFollow>,
    version_warned: bool,
    vanished: bool,
}

impl SessionFollower {
    /// Follow `transcript` from its start.
    #[must_use]
    pub fn new(session: SessionId, transcript: impl Into<PathBuf>) -> Self {
        Self::resume(session, Cursor::start(transcript))
    }

    /// Follow from a saved cursor. Agent files are read from their start the first time; they are
    /// small next to the main transcript.
    #[must_use]
    pub fn resume(session: SessionId, cursor: Cursor) -> Self {
        Self {
            session,
            main: Tailer::resume(cursor),
            identity: Identity::default(),
            cost: None,
            usage_by_message: BTreeMap::new(),
            agents: BTreeMap::new(),
            version_warned: false,
            vanished: false,
        }
    }

    /// The session.
    #[must_use]
    pub fn session(&self) -> &SessionId {
        &self.session
    }

    /// The main transcript path.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.main.path()
    }

    /// The main cursor, for persistence.
    #[must_use]
    pub fn cursor(&self) -> &Cursor {
        self.main.cursor()
    }

    /// What the transcript says about the session.
    #[must_use]
    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// The latest cost record.
    #[must_use]
    pub fn cost(&self) -> Option<&CostState> {
        self.cost.as_ref()
    }

    /// The session's own token usage, max-merged by message id and summed.
    #[must_use]
    pub fn usage(&self) -> Usage {
        sum_usage(self.usage_by_message.values())
    }

    /// The agents seen so far, by id.
    pub fn agents(&self) -> impl Iterator<Item = &AgentState> {
        self.agents.values().map(|a| &a.state)
    }

    /// True once the main transcript was found missing.
    #[must_use]
    pub fn vanished(&self) -> bool {
        self.vanished
    }

    /// Read what is new. `now` dates observations from records without a timestamp.
    pub fn poll(&mut self, now: Timestamp) -> FollowPoll {
        let mut out = FollowPoll::default();
        loop {
            match self.main.poll() {
                Ok(poll) => {
                    if poll.truncated {
                        out.notices
                            .push("transcript shrank; read again from the start".into());
                        self.usage_by_message.clear();
                    }
                    for record in &poll.records {
                        self.absorb_main(record, now, &mut out);
                    }
                    out.records.extend(poll.records);
                    if !poll.more {
                        break;
                    }
                }
                Err(TailError::Vanished(_)) => {
                    if !self.vanished {
                        out.notices.push("transcript vanished".into());
                    }
                    self.vanished = true;
                    out.vanished = true;
                    break;
                }
                Err(e) => {
                    out.notices.push(e.to_string());
                    break;
                }
            }
        }
        self.poll_agents(now, &mut out);
        out
    }

    fn absorb_main(&mut self, record: &Record, now: Timestamp, out: &mut FollowPoll) {
        let at = record.at().unwrap_or(now);
        if let Some(version) = record.version.as_deref()
            && !self.version_warned
            && !version_supported(version)
        {
            self.version_warned = true;
            out.notices.push(format!(
                "transcript written by Claude Code {version}; the parser knows major version {SUPPORTED_MAJOR}"
            ));
        }
        if record.is_content() {
            let before = self.identity.clone();
            set_if_some(&mut self.identity.cwd, record.cwd.as_deref());
            set_if_some(&mut self.identity.git_branch, record.git_branch.as_deref());
            set_if_some(&mut self.identity.version, record.version.as_deref());
            set_if_some(&mut self.identity.slug, record.slug.as_deref());
            set_if_some(
                &mut self.identity.session_kind,
                record.session_kind.as_deref(),
            );
            if before != self.identity {
                out.identity_changed = true;
                out.observations.push(Observation::new(
                    self.session.clone(),
                    at,
                    ObservationBody::Identity {
                        cwd: self.identity.cwd.clone(),
                        name: None,
                        transcript_path: Some(self.main.path().display().to_string()),
                        version: self.identity.version.clone(),
                    },
                ));
            }
        }
        match record.kind.as_str() {
            "ai-title" | "agent-name" => {
                let before = self.identity.clone();
                set_if_some(&mut self.identity.ai_title, record.ai_title.as_deref());
                set_if_some(&mut self.identity.agent_name, record.agent_name.as_deref());
                if before != self.identity {
                    out.identity_changed = true;
                    out.observations.push(Observation::new(
                        self.session.clone(),
                        at,
                        ObservationBody::Identity {
                            cwd: None,
                            name: self
                                .identity
                                .agent_name
                                .clone()
                                .or_else(|| self.identity.ai_title.clone()),
                            transcript_path: None,
                            version: None,
                        },
                    ));
                }
            }
            "permission-mode" => {
                if record.permission_mode != self.identity.permission_mode {
                    self.identity
                        .permission_mode
                        .clone_from(&record.permission_mode);
                    out.identity_changed = true;
                }
            }
            "cost-state" => {
                self.cost = Some(CostState {
                    total_cost_usd: record.total_cost_usd.unwrap_or(0.0),
                    model_usage: record.model_usage.clone().unwrap_or_default(),
                });
            }
            "assistant" => merge_usage(&mut self.usage_by_message, record),
            _ => {}
        }
        for event in events_for(record) {
            out.observations.push(Observation::new(
                self.session.clone(),
                at,
                ObservationBody::Transcript(event),
            ));
        }
    }

    fn poll_agents(&mut self, now: Timestamp, out: &mut FollowPoll) {
        let files = scan_session_dir(self.main.path());
        for file in files.all_agents() {
            let follow = self.agents.entry(file.agent_id.clone()).or_insert_with(|| {
                out.new_agents.push(file.agent_id.clone());
                AgentFollow {
                    tailer: Tailer::new(&file.path),
                    state: AgentState {
                        file: file.clone(),
                        meta: None,
                        usage: Usage::default(),
                        first_activity: None,
                        last_activity: None,
                        model: None,
                    },
                    by_message: BTreeMap::new(),
                    meta_attempts: 0,
                    announced: false,
                }
            });
            follow.state.file.size = file.size;
            follow.state.file.modified = file.modified;
            if file.size != follow.tailer.cursor().offset {
                follow.read_new();
            }
            let had_meta = follow.state.meta.is_some();
            if !had_meta && follow.meta_attempts < META_ATTEMPTS {
                follow.meta_attempts += 1;
                follow.state.meta = file.read_meta();
            }
            // Announce the file once, and again when its meta file turns up later.
            if !follow.announced || (!had_meta && follow.state.meta.is_some()) {
                follow.announced = true;
                let at = follow.state.first_activity.or(file.modified).unwrap_or(now);
                let meta = follow.state.meta.as_ref();
                out.observations.push(Observation::new(
                    self.session.clone(),
                    at,
                    ObservationBody::Transcript(TranscriptEvent::AgentSeen {
                        agent_id: file.agent_id.clone(),
                        agent_type: meta.and_then(|m| m.agent_type.clone()),
                        description: meta.and_then(|m| m.description.clone()),
                        transcript_path: Some(file.path.display().to_string()),
                    }),
                ));
            }
        }
    }
}

fn set_if_some(slot: &mut Option<String>, value: Option<&str>) {
    if let Some(v) = value {
        *slot = Some(v.to_string());
    }
}

/// Keep the field-wise maximum per message id: Claude Code writes one record per content block of
/// a response, each carrying the running usage of that response.
fn merge_usage(by_message: &mut BTreeMap<String, Usage>, record: &Record) {
    let Some(message) = record.message.as_ref() else {
        return;
    };
    let Some(usage) = message.usage.as_ref() else {
        return;
    };
    if message.model.as_deref() == Some("<synthetic>") {
        return;
    }
    let key = message
        .id
        .clone()
        .or_else(|| record.uuid.clone())
        .unwrap_or_default();
    by_message
        .entry(key)
        .and_modify(|u| *u = u.max_with(usage))
        .or_insert_with(|| usage.clone());
}

fn sum_usage<'a>(items: impl Iterator<Item = &'a Usage>) -> Usage {
    items.fold(Usage::default(), |mut acc, u| {
        acc.input_tokens = acc.input_tokens.saturating_add(u.input_tokens);
        acc.cache_creation_input_tokens = acc
            .cache_creation_input_tokens
            .saturating_add(u.cache_creation_input_tokens);
        acc.cache_read_input_tokens = acc
            .cache_read_input_tokens
            .saturating_add(u.cache_read_input_tokens);
        acc.output_tokens = acc.output_tokens.saturating_add(u.output_tokens);
        acc
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use c4_core::{SessionState, SessionTable, StateKind};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the workspace root")
            .join("fixtures")
            .join("home")
            .join("projects")
    }

    fn now() -> Timestamp {
        Timestamp::parse_rfc3339("2026-10-25T14:00:00Z").unwrap()
    }

    fn at(s: &str) -> Option<Timestamp> {
        Timestamp::parse_rfc3339(s)
    }

    #[test]
    fn alpha_folds_into_the_expected_view() {
        let sid = SessionId::new("0a1b2c3d-0001-4000-8000-000000000001");
        let path = fixtures()
            .join("-home-example-proj-alpha")
            .join("0a1b2c3d-0001-4000-8000-000000000001.jsonl");
        let mut follower = SessionFollower::new(sid.clone(), &path);
        let poll = follower.poll(now());
        assert_eq!(poll.records.len(), 15);
        assert!(poll.notices.is_empty(), "{:?}", poll.notices);
        assert_eq!(poll.new_agents, vec!["a0000001".to_string()]);
        assert!(poll.identity_changed);
        assert!(!poll.vanished);

        let id = follower.identity();
        assert_eq!(id.cwd.as_deref(), Some("/home/example/proj-alpha"));
        assert_eq!(id.git_branch.as_deref(), Some("main"));
        assert_eq!(id.version.as_deref(), Some("2.1.299"));
        assert_eq!(id.ai_title.as_deref(), Some("Alpha fixture session"));
        assert_eq!(id.permission_mode.as_deref(), Some("default"));

        // msg_fx_0001 spans two records and is max-merged (12 in, 38 out), then 0002 to 0004.
        let usage = follower.usage();
        assert_eq!(usage.input_tokens, 12 + 4 + 3 + 5);
        assert_eq!(usage.output_tokens, 38 + 71 + 24 + 3);
        assert_eq!(usage.cache_read_input_tokens, 1800 + 1860 + 42_000);
        assert_eq!(usage.cache_creation_input_tokens, 1800 + 60 + 90);
        let cost = follower.cost().unwrap();
        assert!((cost.total_cost_usd - 0.42).abs() < f64::EPSILON);
        assert_eq!(
            cost.model_usage["claude-opus-5-5"].cache_read_input_tokens,
            45_660
        );

        let agents: Vec<&AgentState> = follower.agents().collect();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].usage.output_tokens, 40 + 14);
        assert_eq!(agents[0].usage.input_tokens, 20 + 4);
        assert_eq!(agents[0].model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(agents[0].first_activity, at("2026-10-25T12:00:05.300Z"));
        assert_eq!(agents[0].last_activity, at("2026-10-25T12:00:08Z"));
        assert_eq!(
            agents[0].meta.as_ref().unwrap().agent_type.as_deref(),
            Some("Explore")
        );

        let mut table = SessionTable::new();
        for obs in &poll.observations {
            table.apply(obs, now());
        }
        let view = table.get(&sid).unwrap();
        assert_eq!(
            view.state,
            SessionState::Idle,
            "the last turn record counted no pending agents"
        );
        assert_eq!(view.compactions, 1);
        assert_eq!(view.cwd.as_deref(), Some("/home/example/proj-alpha"));
        assert_eq!(view.version.as_deref(), Some("2.1.299"));
        assert_eq!(view.display_name(), "Alpha fixture session");
        assert_eq!(
            view.transcript_path.as_deref(),
            Some(path.display().to_string().as_str())
        );
        assert_eq!(view.agents.len(), 1);
        assert_eq!(view.agents[0].agent_type.as_deref(), Some("Explore"));
        assert_eq!(
            view.agents[0].description.as_deref(),
            Some("Find build entry points")
        );
        assert!(
            view.agents[0]
                .transcript_path
                .as_deref()
                .unwrap()
                .ends_with("agent-a0000001.jsonl")
        );
        assert_eq!(
            view.agents[0].started_at,
            at("2026-10-25T12:00:05.200Z").unwrap()
        );
        assert_eq!(view.agents[0].ended_at, at("2026-10-25T12:05:01.100Z"));

        // Replaying only up to the compaction shows the background agent still running.
        let mut table = SessionTable::new();
        for obs in poll.observations.iter().take_while(|o| {
            !matches!(
                o.body,
                ObservationBody::Transcript(TranscriptEvent::CompactBoundary { .. })
            )
        }) {
            table.apply(obs, now());
        }
        let view = table.get(&sid).unwrap();
        assert_eq!(view.state.kind(), StateKind::BackgroundAgentsActive);
        assert_eq!(view.running_agents(), 1);

        let second = follower.poll(now());
        assert_eq!(second.records, Vec::new());
        assert_eq!(second.observations.len(), 0);
        assert_eq!(second.new_agents, Vec::<String>::new());
        assert!(!second.identity_changed);
    }

    #[test]
    fn gamma_ends_failed_and_beta_is_mid_tool() {
        let gamma = SessionId::new("0a1b2c3d-0001-4000-8000-000000000003");
        let path = fixtures()
            .join("C--Users-example-proj-gamma")
            .join("0a1b2c3d-0001-4000-8000-000000000003.jsonl");
        let mut f = SessionFollower::new(gamma.clone(), path);
        let poll = f.poll(now());
        let mut table = SessionTable::new();
        for obs in &poll.observations {
            table.apply(obs, now());
        }
        assert_eq!(
            table.get(&gamma).unwrap().state,
            SessionState::Failed {
                error: "API Error: 529 overloaded".into()
            }
        );
        assert_eq!(
            f.usage().input_tokens,
            8,
            "the synthetic error record counts no tokens"
        );
        assert_eq!(
            f.identity().cwd.as_deref(),
            Some("C:\\Users\\example\\proj-gamma")
        );

        let beta = SessionId::new("0a1b2c3d-0001-4000-8000-000000000002");
        let path = fixtures()
            .join("-home-example-proj-beta")
            .join("0a1b2c3d-0001-4000-8000-000000000002.jsonl");
        let mut f = SessionFollower::new(beta.clone(), path);
        let poll = f.poll(now());
        assert_eq!(poll.records.len(), 3, "the partial last line waits");
        let mut table = SessionTable::new();
        for obs in &poll.observations {
            table.apply(obs, now());
        }
        assert_eq!(
            table.get(&beta).unwrap().state,
            SessionState::RunningTool {
                tool: "Bash".into()
            }
        );
        assert_eq!(f.identity().permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(f.identity().git_branch.as_deref(), Some("nightly"));
        assert!(f.agents().next().is_none());
    }

    #[test]
    fn vanished_and_unknown_versions_are_reported_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"user\",\"uuid\":\"u1\",\"version\":\"3.0.0\",\"message\":{\"content\":\"x\"}}\n",
                "{\"type\":\"user\",\"uuid\":\"u2\",\"version\":\"3.0.0\",\"message\":{\"content\":\"y\"}}\n",
            ),
        )
        .unwrap();
        let mut f = SessionFollower::new(SessionId::new("s"), &path);
        let poll = f.poll(now());
        assert_eq!(poll.notices.len(), 1, "{:?}", poll.notices);
        assert!(poll.notices[0].contains("3.0.0"));
        assert_eq!(poll.records.len(), 2);
        assert_eq!(f.poll(now()).notices, Vec::<String>::new());
        std::fs::remove_file(&path).unwrap();
        let gone = f.poll(now());
        assert!(gone.vanished);
        assert_eq!(gone.notices, vec!["transcript vanished".to_string()]);
        assert!(f.poll(now()).notices.is_empty(), "reported once");
        assert!(f.vanished());
    }

    #[test]
    fn an_agent_file_appearing_later_is_announced_with_its_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-10-25T12:00:00Z\",\"message\":{\"content\":\"go\"}}\n",
        )
        .unwrap();
        let mut f = SessionFollower::new(SessionId::new("s"), &path);
        let first = f.poll(now());
        assert_eq!(first.new_agents, Vec::<String>::new());

        let sub = dir.path().join("s").join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("agent-zz.jsonl"),
            "{\"type\":\"user\",\"uuid\":\"z1\",\"timestamp\":\"2026-10-25T12:00:09Z\",\"isSidechain\":true,\"agentId\":\"zz\",\"message\":{\"content\":\"task\"}}\n",
        )
        .unwrap();
        let second = f.poll(now());
        assert_eq!(second.new_agents, vec!["zz".to_string()]);
        let seen: Vec<&Observation> = second
            .observations
            .iter()
            .filter(|o| {
                matches!(
                    o.body,
                    ObservationBody::Transcript(TranscriptEvent::AgentSeen { .. })
                )
            })
            .collect();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].at, at("2026-10-25T12:00:09Z").unwrap());
        assert!(matches!(
            &seen[0].body,
            ObservationBody::Transcript(TranscriptEvent::AgentSeen {
                agent_type: None,
                ..
            })
        ));

        // The meta file lands after the transcript: the agent is announced again, with its type.
        std::fs::write(
            sub.join("agent-zz.meta.json"),
            "{\"agentType\":\"Plan\",\"description\":\"plan it\",\"spawnDepth\":1}",
        )
        .unwrap();
        let third = f.poll(now());
        assert_eq!(third.new_agents, Vec::<String>::new());
        assert!(matches!(
            &third.observations[0].body,
            ObservationBody::Transcript(TranscriptEvent::AgentSeen { agent_type: Some(t), .. }) if t == "Plan"
        ));
        assert!(
            f.poll(now()).observations.is_empty(),
            "nothing new, nothing repeated"
        );
    }
}
