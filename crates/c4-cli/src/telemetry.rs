// <wtl> project: c4 | file: crates/c4-cli/src/telemetry.rs | author: watchthelight </wtl>
//! `c4 telemetry`: what one transcript file records. The main file is read once from the start,
//! the agent files beside it are read too, and the records are folded through the same state
//! machine the sidebar uses. Everything printed comes from the files. The recorded state is what
//! the file says happened, which is history: it says nothing about whether a process is alive, and
//! this command never looks for one.

use crate::OUTPUT_SCHEMA;
use crate::args::TelemetryArgs;
use c4_core::redact::redact;
use c4_core::{SessionId, SessionTable, SessionView, Timestamp};
use c4_transcript::{AgentState, CostState, FollowPoll, Identity, SessionFollower, Usage};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

/// The sentence every report carries next to its recorded state.
pub const RECORDED_NOTE: &str =
    "folded from the transcript alone: what the file recorded, not whether a process is alive";

/// Token counts, summed the way the status line sums them.
#[derive(Serialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct Tokens {
    /// Fresh input tokens.
    pub input: u64,
    /// Tokens written to the prompt cache.
    pub cache_creation: u64,
    /// Tokens read from the prompt cache.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
    /// Fresh plus cache writes plus cache reads.
    pub total_input: u64,
}

impl From<&Usage> for Tokens {
    fn from(u: &Usage) -> Self {
        Self {
            input: u.input_tokens,
            cache_creation: u.cache_creation_input_tokens,
            cache_read: u.cache_read_input_tokens,
            output: u.output_tokens,
            total_input: u.total_input(),
        }
    }
}

impl Tokens {
    fn add(&mut self, other: &Tokens) {
        self.input = self.input.saturating_add(other.input);
        self.cache_creation = self.cache_creation.saturating_add(other.cache_creation);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.output = self.output.saturating_add(other.output);
        self.total_input = self.total_input.saturating_add(other.total_input);
    }
}

/// What the main file held.
#[derive(Serialize, Debug)]
pub struct Records {
    /// Complete lines read.
    pub lines: u64,
    /// Lines that were records.
    pub parsed: usize,
    /// Lines that were not JSON objects.
    pub skipped: u64,
    /// Records per `type`.
    pub by_type: BTreeMap<String, usize>,
    /// The earliest record timestamp.
    pub first_at: Option<Timestamp>,
    /// The latest record timestamp.
    pub last_at: Option<Timestamp>,
}

/// What the file says about the session.
#[derive(Serialize, Debug)]
pub struct IdentityOut {
    /// The working directory of the latest content record.
    pub cwd: Option<String>,
    /// The git branch of the latest content record.
    pub git_branch: Option<String>,
    /// The Claude Code version of the latest content record.
    pub version: Option<String>,
    /// The latest AI title.
    pub ai_title: Option<String>,
    /// The latest name set with `--name` or `/rename`.
    pub agent_name: Option<String>,
    /// The latest permission mode.
    pub permission_mode: Option<String>,
    /// The plan slug.
    pub slug: Option<String>,
    /// `bg` for background sessions.
    pub session_kind: Option<String>,
}

/// One model's share of the recorded cost.
#[derive(Serialize, Debug)]
pub struct ModelCost {
    /// Fresh input tokens.
    pub input: u64,
    /// Output tokens.
    pub output: u64,
    /// Thinking tokens.
    pub thinking: u64,
    /// Cache reads.
    pub cache_read: u64,
    /// Cache writes.
    pub cache_creation: u64,
    /// Dollars.
    pub cost_usd: f64,
}

/// The latest `cost-state` record.
#[derive(Serialize, Debug)]
pub struct Cost {
    /// Dollars in total.
    pub total_usd: f64,
    /// Per model.
    pub by_model: BTreeMap<String, ModelCost>,
}

/// One agent file.
#[derive(Serialize, Debug)]
pub struct AgentOut {
    /// The agent id.
    pub id: String,
    /// The agent type from its meta file.
    pub agent_type: Option<String>,
    /// The description from its meta file.
    pub description: Option<String>,
    /// The workflow run it belongs to, for workflow agents.
    pub workflow: Option<String>,
    /// The model of its latest assistant record.
    pub model: Option<String>,
    /// Its own tokens.
    pub usage: Tokens,
    /// The earliest record time.
    pub first_activity: Option<Timestamp>,
    /// The latest record time.
    pub last_activity: Option<Timestamp>,
    /// The file.
    pub file: String,
    /// Its size.
    pub bytes: u64,
}

/// The fold of the records through the state machine.
#[derive(Serialize, Debug)]
pub struct Recorded {
    /// The state, described.
    pub state: String,
    /// The state kind label.
    pub kind: &'static str,
    /// Whether the last transition was an observed fact.
    pub observed: bool,
    /// Compactions recorded.
    pub compactions: u32,
    /// Agents the file and its directory mention.
    pub agents_seen: usize,
    /// Agents that had started and not stopped when the file ends.
    pub agents_running_at_end: usize,
    /// `pendingBackgroundAgentCount` of the last turn record.
    pub pending_background_agents: u32,
    /// [`RECORDED_NOTE`].
    pub note: &'static str,
}

/// The report.
#[derive(Serialize, Debug)]
pub struct Report {
    /// The output schema version.
    pub schema: u32,
    /// The main transcript file.
    pub transcript: String,
    /// The session id, from the file name.
    pub session: String,
    /// What the main file held.
    pub records: Records,
    /// What the file says about the session.
    pub identity: IdentityOut,
    /// The session's own tokens.
    pub usage: Tokens,
    /// The latest cost record, when there is one.
    pub cost: Option<Cost>,
    /// The agent files, in id order.
    pub agents: Vec<AgentOut>,
    /// The agents' tokens summed.
    pub agents_usage: Tokens,
    /// The fold of the records.
    pub recorded: Recorded,
    /// Problems the reader noticed.
    pub notices: Vec<String>,
}

/// Run the command.
pub fn run(args: &TelemetryArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let report = read(&args.transcript)?;
    if args.json {
        writeln!(out, "{}", serde_json::to_string_pretty(&report)?)?;
    } else {
        write_text(out, &report)?;
    }
    out.flush()?;
    Ok(())
}

/// Read one transcript and the agent files beside it, from the start.
pub fn read(path: &Path) -> anyhow::Result<Report> {
    if !path.is_file() {
        anyhow::bail!("no transcript file at {}", path.display());
    }
    let session = path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .map(SessionId::new)
        .ok_or_else(|| anyhow::anyhow!("no session id in the file name {}", path.display()))?;
    let mut follower = SessionFollower::new(session.clone(), path);
    let now = Timestamp::now();
    let poll = follower.poll(now);
    let mut table = SessionTable::new();
    for obs in &poll.observations {
        table.apply(obs, now);
    }
    Ok(build(path, &session, &follower, &poll, table.get(&session)))
}

fn build(
    path: &Path,
    session: &SessionId,
    follower: &SessionFollower,
    poll: &FollowPoll,
    view: Option<&SessionView>,
) -> Report {
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let mut first_at: Option<Timestamp> = None;
    let mut last_at: Option<Timestamp> = None;
    for record in &poll.records {
        *by_type.entry(record.kind.clone()).or_insert(0) += 1;
        if let Some(at) = record.at() {
            if first_at.is_none_or(|f| at < f) {
                first_at = Some(at);
            }
            if last_at.is_none_or(|l| at > l) {
                last_at = Some(at);
            }
        }
    }
    let cursor = follower.cursor();
    let agents: Vec<AgentOut> = follower.agents().map(agent_out).collect();
    let mut agents_usage = Tokens::default();
    for agent in &agents {
        agents_usage.add(&agent.usage);
    }
    let recorded = view.map_or_else(
        || Recorded {
            state: "nothing recorded".into(),
            kind: "unknown",
            observed: false,
            compactions: 0,
            agents_seen: 0,
            agents_running_at_end: 0,
            pending_background_agents: 0,
            note: RECORDED_NOTE,
        },
        |v| Recorded {
            state: v.state.describe(),
            kind: v.state.kind().label(),
            observed: v.evidence.is_observed(),
            compactions: v.compactions,
            agents_seen: v.agents.len(),
            agents_running_at_end: v.running_agents(),
            pending_background_agents: v.pending_background_agents,
            note: RECORDED_NOTE,
        },
    );
    Report {
        schema: OUTPUT_SCHEMA,
        transcript: path.display().to_string(),
        session: session.as_str().to_string(),
        records: Records {
            lines: cursor.lines,
            parsed: poll.records.len(),
            skipped: cursor.skipped,
            by_type,
            first_at,
            last_at,
        },
        identity: identity_out(follower.identity()),
        usage: Tokens::from(&follower.usage()),
        cost: follower.cost().map(cost_out),
        agents,
        agents_usage,
        recorded,
        notices: poll.notices.clone(),
    }
}

fn identity_out(id: &Identity) -> IdentityOut {
    IdentityOut {
        cwd: id.cwd.clone(),
        git_branch: id.git_branch.clone(),
        version: id.version.clone(),
        ai_title: id.ai_title.clone(),
        agent_name: id.agent_name.clone(),
        permission_mode: id.permission_mode.clone(),
        slug: id.slug.clone(),
        session_kind: id.session_kind.clone(),
    }
}

fn cost_out(c: &CostState) -> Cost {
    Cost {
        total_usd: c.total_cost_usd,
        by_model: c
            .model_usage
            .iter()
            .map(|(model, u)| {
                (
                    model.clone(),
                    ModelCost {
                        input: u.input_tokens,
                        output: u.output_tokens,
                        thinking: u.thinking_tokens,
                        cache_read: u.cache_read_input_tokens,
                        cache_creation: u.cache_creation_input_tokens,
                        cost_usd: u.cost_usd,
                    },
                )
            })
            .collect(),
    }
}

fn agent_out(a: &AgentState) -> AgentOut {
    let meta = a.meta.as_ref();
    AgentOut {
        id: a.file.agent_id.clone(),
        agent_type: meta.and_then(|m| m.agent_type.clone()),
        description: meta.and_then(|m| m.description.clone()),
        workflow: a.file.workflow.clone(),
        model: a.model.clone(),
        usage: Tokens::from(&a.usage),
        first_activity: a.first_activity,
        last_activity: a.last_activity,
        file: a.file.path.display().to_string(),
        bytes: a.file.size,
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn write_text(out: &mut dyn Write, r: &Report) -> std::io::Result<()> {
    writeln!(out, "transcript: {}", redact(&r.transcript))?;
    writeln!(out, "session: {}", r.session)?;
    let types: Vec<String> = r
        .records
        .by_type
        .iter()
        .map(|(kind, n)| format!("{kind} {n}"))
        .collect();
    writeln!(
        out,
        "records: {} lines, {} parsed, {} skipped ({})",
        r.records.lines,
        r.records.parsed,
        r.records.skipped,
        types.join(", ")
    )?;
    if let (Some(first), Some(last)) = (r.records.first_at, r.records.last_at) {
        writeln!(out, "  from {first} to {last}")?;
    }
    let id = &r.identity;
    writeln!(
        out,
        "identity: cwd {}, branch {}, version {}, mode {}",
        redact(id.cwd.as_deref().unwrap_or("-")),
        id.git_branch.as_deref().unwrap_or("-"),
        id.version.as_deref().unwrap_or("-"),
        id.permission_mode.as_deref().unwrap_or("-")
    )?;
    if let Some(name) = id.agent_name.as_deref().or(id.ai_title.as_deref()) {
        writeln!(out, "  name: {}", redact(name))?;
    }
    writeln!(
        out,
        "usage: input {}, cache write {}, cache read {}, output {} (total input {})",
        r.usage.input,
        r.usage.cache_creation,
        r.usage.cache_read,
        r.usage.output,
        r.usage.total_input
    )?;
    match &r.cost {
        Some(c) => {
            writeln!(out, "cost: ${:.2}", c.total_usd)?;
            for (model, u) in &c.by_model {
                writeln!(
                    out,
                    "  {model}: ${:.2}, input {}, output {}, cache read {}, cache write {}",
                    u.cost_usd, u.input, u.output, u.cache_read, u.cache_creation
                )?;
            }
        }
        None => writeln!(out, "cost: not recorded")?,
    }
    writeln!(out, "agents: {}", r.agents.len())?;
    for a in &r.agents {
        let description = a
            .description
            .as_deref()
            .map_or(String::new(), |d| format!(" \"{}\"", redact(d)));
        writeln!(
            out,
            "  {} {}{}: model {}, input {}, output {}, cache read {}",
            a.id,
            a.agent_type.as_deref().unwrap_or("agent"),
            description,
            a.model.as_deref().unwrap_or("-"),
            a.usage.input,
            a.usage.output,
            a.usage.cache_read
        )?;
    }
    if !r.agents.is_empty() {
        writeln!(
            out,
            "  agents total: input {}, output {}, cache read {}",
            r.agents_usage.input, r.agents_usage.output, r.agents_usage.cache_read
        )?;
    }
    writeln!(
        out,
        "recorded state: {} ({}), {} compaction{}, {} agent{} seen, {} running at the end",
        r.recorded.state,
        if r.recorded.observed {
            "observed"
        } else {
            "heuristic"
        },
        r.recorded.compactions,
        plural(r.recorded.compactions as usize),
        r.recorded.agents_seen,
        plural(r.recorded.agents_seen),
        r.recorded.agents_running_at_end
    )?;
    writeln!(out, "  {}", r.recorded.note)?;
    for notice in &r.notices {
        writeln!(out, "notice: {}", redact(notice))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the workspace root")
            .join("fixtures")
            .join("home")
            .join("projects")
            .join("-home-example-proj-alpha")
            .join("0a1b2c3d-0001-4000-8000-000000000001.jsonl")
    }

    #[test]
    fn the_alpha_fixture_adds_up() {
        let r = read(&alpha()).unwrap();
        assert_eq!(r.session, "0a1b2c3d-0001-4000-8000-000000000001");
        assert_eq!(r.records.parsed, 15);
        assert_eq!(r.records.skipped, 0);
        assert_eq!(r.usage.input, 24);
        assert_eq!(r.usage.output, 136);
        assert_eq!(r.usage.cache_read, 45_660);
        assert_eq!(r.usage.cache_creation, 1950);
        assert_eq!(r.usage.total_input, 24 + 45_660 + 1950);
        let cost = r.cost.as_ref().unwrap();
        assert!((cost.total_usd - 0.42).abs() < 1e-9);
        assert_eq!(r.agents.len(), 1);
        assert_eq!(r.agents[0].agent_type.as_deref(), Some("Explore"));
        assert_eq!(r.agents[0].usage.output, 54);
        assert_eq!(r.agents_usage.output, 54);
        assert_eq!(r.recorded.kind, "idle");
        assert_eq!(r.recorded.compactions, 1);
        assert_eq!(r.recorded.agents_seen, 1);
        assert_eq!(r.recorded.agents_running_at_end, 0);
        assert_eq!(r.identity.version.as_deref(), Some("2.1.299"));
        assert!(r.notices.is_empty(), "{:?}", r.notices);

        let mut text = Vec::new();
        write_text(&mut text, &r).unwrap();
        let text = String::from_utf8(text).unwrap();
        assert!(text.contains("usage: input 24, cache write 1950, cache read 45660, output 136"));
        assert!(text.contains("cost: $0.42"));
        assert!(text.contains(RECORDED_NOTE));
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let err = read(Path::new("no-such-dir/none.jsonl")).unwrap_err();
        assert!(err.to_string().contains("no transcript file"));
    }
}
