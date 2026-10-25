// <wtl> project: c4 | file: crates/c4-core/src/lib.rs | author: watchthelight </wtl>
//! Shared vocabulary for C4: session identity, the nine sidebar states, the evidence that backs every
//! transition, and the versioned event envelope that crosses process boundaries.
//!
//! Nothing in this crate touches the filesystem or the network. Readers in `c4-discovery`,
//! `c4-transcript` and `c4-ingest` turn what they see into [`Observation`]s; `reduce` folds them into
//! [`SessionView`]s; the desktop app renders those.

/// The crate version, exported so the CLI and the UI can show the same number.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
