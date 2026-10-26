// <wtl> project: c4 | file: crates/c4-core/src/envelope.rs | author: watchthelight </wtl>
//! The versioned envelope that carries events between the core, the session hosts, the shim and
//! the window. Every message says which schema it speaks, so a newer C4 can read an older spool.

use crate::clock::Timestamp;
use crate::evidence::Evidence;
use crate::ids::SessionId;
use crate::state::SessionState;
use crate::view::SessionView;
use serde::{Deserialize, Serialize};

/// The schema version written into every envelope. Bump when a payload changes shape.
pub const SCHEMA_VERSION: u16 = 1;

/// A message with its schema version, sequence number and time.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
pub struct Envelope<T> {
    /// The schema version.
    pub v: u16,
    /// A per-producer sequence number; gaps mean something was dropped.
    pub seq: u64,
    /// When the producer sent it.
    pub at: Timestamp,
    /// The payload.
    pub body: T,
}

impl<T> Envelope<T> {
    /// Wrap a payload.
    #[must_use]
    pub fn new(seq: u64, at: Timestamp, body: T) -> Self {
        Self {
            v: SCHEMA_VERSION,
            seq,
            at,
            body,
        }
    }
}

/// How loud a notice is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    /// Informational.
    Info,
    /// Something degraded (telemetry missing, a drop counter rising).
    Warning,
    /// Something failed.
    Error,
}

/// What the core publishes to the UI.
#[derive(Clone, PartialEq, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum Event {
    /// A session appeared or changed. Consumers compare `seq` to skip unchanged views.
    SessionUpserted {
        /// The view.
        view: Box<SessionView>,
    },
    /// A session left the list (its retention window passed).
    SessionRemoved {
        /// The id.
        id: SessionId,
    },
    /// The state of a session changed.
    Transition {
        /// The id.
        id: SessionId,
        /// Before.
        from: SessionState,
        /// After.
        to: SessionState,
        /// Why.
        evidence: Evidence,
    },
    /// A discovery pass finished.
    DiscoveryPass {
        /// Sessions found.
        sessions: usize,
        /// How long it took.
        elapsed_ms: u64,
    },
    /// Producers dropped items because a queue was full.
    Dropped {
        /// Which queue.
        queue: String,
        /// How many since the last report.
        count: u64,
    },
    /// Something the user should see.
    Notice {
        /// How loud.
        level: NoticeLevel,
        /// The text.
        text: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trip() {
        let e = Envelope::new(
            7,
            Timestamp::from_millis(9),
            Event::Notice {
                level: NoticeLevel::Info,
                text: "hi".into(),
            },
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.starts_with("{\"v\":1,\"seq\":7,\"at\":9,"));
        let back: Envelope<Event> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }
}
