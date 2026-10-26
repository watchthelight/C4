// <wtl> project: c4 | file: crates/c4-testkit/src/lib.rs | author: watchthelight </wtl>
//! Test harness for the crates that read a Claude home. Nothing here touches `~/.claude`: every
//! helper works inside a [`ScratchHome`], a temporary directory laid out like Claude Code's own
//! config root, and the stand-in process in [`FakeClaude`] is a child this crate spawns and ends
//! itself. No process that was already running is ever signaled.
//!
//! - [`ScratchHome`]: an empty home, or one seeded from `fixtures/home`, with writers for registry
//!   records, transcripts and agent files.
//! - [`FakeClaude`]: a long-lived child registered in a scratch home the way Claude Code registers
//!   itself, `sessions/<pid>.json` with the child's real pid and creation time, so the discovery
//!   crate's liveness ladder sees a live session. Its registry status and name can change, and it
//!   can exit cleanly (the record disappears, as Claude Code's does) or crash (the record stays).
//!
//! The harness is a regular library so that the integration tests of other crates can depend on it.
//! `scripts/fake_claude.py` is the same stand-in as a script, for shell and manual checks.

pub mod fake;
pub mod home;

pub use fake::{FAKE_VERSION, FakeClaude, FakeClaudeOptions};
pub use home::{ScratchHome, fixtures_root};
