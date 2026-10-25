// <wtl> project: c4 | file: crates/c4-core/src/evidence.rs | author: watchthelight </wtl>
//! Every state a session is shown in carries the evidence that produced it: where it came from,
//! whether it was observed or guessed, and when.

use crate::clock::Timestamp;
use crate::ids::HostKind;
use serde::{Deserialize, Serialize};

/// Observed facts come from Claude Code itself or from the operating system. Heuristics are C4's own
/// inferences, shown with a `~` marker and never allowed to override a fresh observed fact.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    /// Reported by Claude Code, a hook payload, the terminal protocol or the process table.
    Observed,
    /// Inferred by C4 from timing or indirect signs.
    Heuristic,
}

/// Where a fact came from.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "source", content = "detail")]
pub enum Source {
    /// `~/.claude/sessions/<pid>.json`.
    Registry,
    /// `claude agents --json`.
    AgentsJson,
    /// A hook event, named.
    Hook(String),
    /// The status line payload.
    Statusline,
    /// A transcript record.
    Transcript,
    /// The OSC 7501 program status protocol.
    Osc7501,
    /// An iTerm2-style OSC 21337 status report.
    Osc21337,
    /// The process table.
    Process,
    /// The background session supervisor files.
    Daemon,
    /// A terminal host adapter.
    Host(HostKind),
    /// C4's own managed session host.
    Managed,
    /// A clock tick inside C4 (stall detection and the like).
    Timer,
}

impl Source {
    /// A short label for the UI and logs.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Source::Registry => "registry".into(),
            Source::AgentsJson => "claude agents".into(),
            Source::Hook(name) => format!("hook {name}"),
            Source::Statusline => "status line".into(),
            Source::Transcript => "transcript".into(),
            Source::Osc7501 => "OSC 7501".into(),
            Source::Osc21337 => "OSC 21337".into(),
            Source::Process => "process table".into(),
            Source::Daemon => "background supervisor".into(),
            Source::Host(host) => host.label().into(),
            Source::Managed => "managed host".into(),
            Source::Timer => "timer".into(),
        }
    }
}

/// The provenance of a state.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
pub struct Evidence {
    /// Where the fact came from.
    pub source: Source,
    /// Observed or guessed.
    pub certainty: Certainty,
    /// When the fact was true, in the source's own clock when it has one.
    pub at: Timestamp,
}

impl Evidence {
    /// An observed fact.
    #[must_use]
    pub fn observed(source: Source, at: Timestamp) -> Self {
        Self {
            source,
            certainty: Certainty::Observed,
            at,
        }
    }

    /// A guess.
    #[must_use]
    pub fn heuristic(source: Source, at: Timestamp) -> Self {
        Self {
            source,
            certainty: Certainty::Heuristic,
            at,
        }
    }

    /// True for observed facts.
    #[must_use]
    pub fn is_observed(&self) -> bool {
        self.certainty == Certainty::Observed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_tagged() {
        let e = Evidence::observed(Source::Hook("Stop".into()), Timestamp::from_millis(5));
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["source"]["source"], "hook");
        assert_eq!(json["source"]["detail"], "Stop");
        assert_eq!(json["certainty"], "observed");
        let back: Evidence = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
        assert_eq!(e.source.label(), "hook Stop");
    }
}
