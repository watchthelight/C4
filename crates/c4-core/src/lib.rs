// <wtl> project: c4 | file: crates/c4-core/src/lib.rs | author: watchthelight </wtl>
//! Shared vocabulary for C4: session identity, the nine sidebar states, the evidence that backs every
//! transition, the observations that feed the state machine, and the versioned event envelope that
//! crosses process boundaries.
//!
//! Nothing in this crate touches the filesystem or the network. Readers in `c4-discovery`,
//! `c4-transcript` and `c4-ingest` turn what they see into [`Observation`]s; [`reduce`] folds them
//! into [`SessionView`]s; the desktop app renders those.

pub mod clock;
pub mod envelope;
pub mod evidence;
pub mod ids;
pub mod observation;
pub mod redact;
pub mod reduce;
pub mod state;
pub mod table;
pub mod view;

pub use clock::Timestamp;
pub use envelope::{Envelope, Event, NoticeLevel, SCHEMA_VERSION};
pub use evidence::{Certainty, Evidence, Source};
pub use ids::{AgentId, HostKind, ProcessRef, SessionId, SessionKind};
pub use observation::{
    Attachment, BackgroundTask, BgState, HookEvent, Observation, ObservationBody, OscKind,
    OscState, RegistryStatus, TranscriptEvent,
};
pub use reduce::{HEURISTIC_GUARD_MS, STALL_AFTER_MS, Transition, reduce, tick};
pub use state::{DisconnectReason, InputReason, SessionState, StateKind};
pub use table::SessionTable;
pub use view::{AgentInfo, SessionView, TaskInfo, ToolInfo};

/// The crate version, exported so the CLI and the UI can show the same number.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
