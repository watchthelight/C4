// <wtl> project: c4 | file: crates/c4-transcript/src/scan.rs | author: watchthelight </wtl>
//! The directory beside a transcript: `<session>/subagents/agent-<id>.jsonl` with its
//! `agent-<id>.meta.json`, `<session>/subagents/workflows/wf_<id>/agent-*.jsonl` with a
//! `journal.jsonl`, and `<session>/workflows/wf_<id>.json`. Only names, sizes and times are read
//! here; the transcripts go through the tailer and the small meta files are read on request.

use c4_core::Timestamp;
use serde::Deserialize;
use std::ffi::OsStr;
use std::fs::DirEntry;
use std::path::{Path, PathBuf};

/// `agent-<id>.meta.json`.
#[derive(Clone, PartialEq, Eq, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentMeta {
    /// The agent definition used (`Explore`, `Plan`, `general-purpose`, `workflow-subagent`).
    #[serde(default)]
    pub agent_type: Option<String>,
    /// The description the caller gave.
    #[serde(default)]
    pub description: Option<String>,
    /// The `tool_use` in the parent transcript that spawned it.
    #[serde(default)]
    pub tool_use_id: Option<String>,
    /// Nesting depth; 1 for a direct child of the main session.
    #[serde(default)]
    pub spawn_depth: Option<u32>,
    /// The request shape, when recorded.
    #[serde(default)]
    pub request_shape: Option<String>,
}

/// One agent transcript file.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AgentFile {
    /// The agent id from the file name.
    pub agent_id: String,
    /// The transcript.
    pub path: PathBuf,
    /// Its size.
    pub size: u64,
    /// When it was last written.
    pub modified: Option<Timestamp>,
    /// The workflow run it belongs to, for files under `subagents/workflows/`.
    pub workflow: Option<String>,
}

impl AgentFile {
    /// The `.meta.json` beside the transcript.
    #[must_use]
    pub fn meta_path(&self) -> PathBuf {
        self.path.with_extension("meta.json")
    }

    /// Read the meta file. `None` when it is missing or not JSON.
    #[must_use]
    pub fn read_meta(&self) -> Option<AgentMeta> {
        let text = std::fs::read_to_string(self.meta_path()).ok()?;
        serde_json::from_str(&text).ok()
    }
}

/// One workflow run directory.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WorkflowFiles {
    /// The run id (`wf_...`).
    pub run_id: String,
    /// The directory.
    pub dir: PathBuf,
    /// The agents of the run.
    pub agents: Vec<AgentFile>,
    /// The `journal.jsonl`, when present.
    pub journal: Option<PathBuf>,
    /// The `<session>/workflows/wf_<id>.json` state file, when present.
    pub state: Option<PathBuf>,
}

/// Everything found for one session.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct SessionFiles {
    /// Direct subagents, by agent id.
    pub agents: Vec<AgentFile>,
    /// Workflow runs, by run id.
    pub workflows: Vec<WorkflowFiles>,
}

impl SessionFiles {
    /// Every agent file, direct and workflow, by agent id.
    pub fn all_agents(&self) -> impl Iterator<Item = &AgentFile> {
        self.agents
            .iter()
            .chain(self.workflows.iter().flat_map(|w| w.agents.iter()))
    }
}

/// The directory beside a transcript: the path without `.jsonl`.
#[must_use]
pub fn session_dir(transcript: &Path) -> PathBuf {
    transcript.with_extension("")
}

fn agent_id_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_suffix(".jsonl")?;
    let id = stem.strip_prefix("agent-")?;
    (!id.is_empty()).then(|| id.to_string())
}

fn agent_file(entry: &DirEntry, workflow: Option<&str>) -> Option<AgentFile> {
    let path = entry.path();
    let agent_id = agent_id_of(&path)?;
    let meta = entry.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    Some(AgentFile {
        agent_id,
        path,
        size: meta.len(),
        modified: meta.modified().ok().and_then(Timestamp::from_system_time),
        workflow: workflow.map(str::to_string),
    })
}

fn agent_files(dir: &Path, workflow: Option<&str>) -> Vec<AgentFile> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<AgentFile> = read
        .flatten()
        .filter_map(|entry| agent_file(&entry, workflow))
        .collect();
    out.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));
    out
}

/// Scan the directory beside `transcript`. A session without one gives an empty result.
#[must_use]
pub fn scan_session_dir(transcript: &Path) -> SessionFiles {
    let dir = session_dir(transcript);
    let subagents = dir.join("subagents");
    let mut files = SessionFiles {
        agents: agent_files(&subagents, None),
        workflows: Vec::new(),
    };
    let workflows_dir = subagents.join("workflows");
    if let Ok(read) = std::fs::read_dir(&workflows_dir) {
        for entry in read.flatten() {
            let run_dir = entry.path();
            if !run_dir.is_dir() {
                continue;
            }
            let Some(run_id) = run_dir
                .file_name()
                .and_then(OsStr::to_str)
                .map(str::to_string)
            else {
                continue;
            };
            let agents = agent_files(&run_dir, Some(&run_id));
            let journal = run_dir.join("journal.jsonl");
            let state = dir.join("workflows").join(format!("{run_id}.json"));
            files.workflows.push(WorkflowFiles {
                run_id,
                dir: run_dir,
                agents,
                journal: journal.is_file().then_some(journal),
                state: state.is_file().then_some(state),
            });
        }
        files.workflows.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the workspace root")
            .join("fixtures")
            .join("home")
            .join("projects")
    }

    #[test]
    fn finds_the_alpha_subagent_and_its_meta() {
        let transcript = fixtures()
            .join("-home-example-proj-alpha")
            .join("0a1b2c3d-0001-4000-8000-000000000001.jsonl");
        let files = scan_session_dir(&transcript);
        assert_eq!(files.agents.len(), 1);
        let a = &files.agents[0];
        assert_eq!(a.agent_id, "a0000001");
        assert_eq!(a.size, 2409);
        assert!(a.modified.is_some());
        assert!(a.meta_path().ends_with("agent-a0000001.meta.json"));
        let meta = a.read_meta().unwrap();
        assert_eq!(meta.agent_type.as_deref(), Some("Explore"));
        assert_eq!(meta.description.as_deref(), Some("Find build entry points"));
        assert_eq!(meta.tool_use_id.as_deref(), Some("toolu_fx_0002"));
        assert_eq!(meta.spawn_depth, Some(1));
        assert_eq!(files.workflows, Vec::new());
        assert_eq!(files.all_agents().count(), 1);
    }

    #[test]
    fn sessions_without_a_directory_are_empty() {
        let transcript = fixtures()
            .join("-home-example-proj-beta")
            .join("0a1b2c3d-0001-4000-8000-000000000002.jsonl");
        assert_eq!(scan_session_dir(&transcript), SessionFiles::default());
    }

    #[test]
    fn workflow_runs_and_odd_names() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("s.jsonl");
        let sub = dir.path().join("s").join("subagents");
        let run = sub.join("workflows").join("wf_abc");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::create_dir_all(dir.path().join("s").join("workflows")).unwrap();
        std::fs::write(sub.join("agent-one.jsonl"), "").unwrap();
        std::fs::write(sub.join("agent-one.meta.json"), "{bad json").unwrap();
        std::fs::write(sub.join("agent-.jsonl"), "").unwrap();
        std::fs::write(sub.join("notes.txt"), "").unwrap();
        std::fs::write(run.join("agent-w1.jsonl"), "").unwrap();
        std::fs::write(run.join("journal.jsonl"), "").unwrap();
        std::fs::write(
            dir.path().join("s").join("workflows").join("wf_abc.json"),
            "{}",
        )
        .unwrap();
        let files = scan_session_dir(&transcript);
        assert_eq!(
            files.agents.len(),
            1,
            "agent-.jsonl and notes.txt are not agents"
        );
        assert_eq!(files.agents[0].agent_id, "one");
        assert!(
            files.agents[0].read_meta().is_none(),
            "a bad meta file is ignored"
        );
        assert_eq!(files.workflows.len(), 1);
        let w = &files.workflows[0];
        assert_eq!(w.run_id, "wf_abc");
        assert_eq!(w.agents[0].agent_id, "w1");
        assert_eq!(w.agents[0].workflow.as_deref(), Some("wf_abc"));
        assert!(w.agents[0].read_meta().is_none(), "no meta file at all");
        assert!(w.journal.is_some());
        assert!(w.state.is_some());
        assert_eq!(files.all_agents().count(), 2);
        assert_eq!(
            session_dir(Path::new("/a/b/s.jsonl")),
            PathBuf::from("/a/b/s")
        );
    }
}
