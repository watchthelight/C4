// <wtl> project: c4 | file: crates/c4-transcript/src/lib.rs | author: watchthelight </wtl>
//! Reads Claude Code transcripts (`~/.claude/projects/<encoded cwd>/<session>.jsonl` and the
//! `<session>/subagents/` directory beside it) without ever reading a file twice.
//!
//! - [`tailer`]: a byte-offset tailer. It reads from the last offset, hands back complete lines
//!   only, and leaves a partial last line for the next poll. A file that shrank was rewritten and is
//!   read again from the start; a file that vanished (the 30-day cleanup) is reported, not retried.
//! - [`record`]: the tolerant record parser. Every field is optional, unknown fields are ignored,
//!   and a line that is not JSON is counted and skipped.
//! - [`events`]: the map from a record to the [`c4_core::TranscriptEvent`]s the state machine
//!   consumes.
//! - [`scan`]: the subagent and workflow directory scanner (file names, sizes and `.meta.json`).
//! - [`index`]: the project index, built from file metadata alone.
//! - [`follow`]: one session's main transcript and its agent files, folded into observations.
//! - [`watch`]: a file watcher that says which transcript changed, so the follower polls on demand
//!   instead of on a timer.
//!
//! Nothing here writes to the Claude home, and nothing here keeps prompt text beyond the records it
//! hands back to the caller.

pub mod events;
pub mod follow;
pub mod index;
pub mod record;
pub mod scan;
pub mod tailer;
pub mod watch;

pub use events::events_for;
pub use follow::{AgentState, CostState, FollowPoll, Identity, SessionFollower};
pub use index::{IndexEntry, ProjectIndex, index_projects};
pub use record::{
    Block, CompactMetadata, Content, Message, ModelUsage, Record, SUPPORTED_MAJOR, Usage,
    version_supported,
};
pub use scan::{AgentFile, AgentMeta, SessionFiles, WorkflowFiles, scan_session_dir, session_dir};
pub use tailer::{Cursor, MAX_CHUNK, Poll, TailError, Tailer};
pub use watch::{Change, ChangeKind, TranscriptWatcher, is_transcript_path};
