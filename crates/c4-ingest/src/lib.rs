// <wtl> project: c4 | file: crates/c4-ingest/src/lib.rs | author: watchthelight </wtl>
//! The receiving end of `c4-shim`: a server on the ingest endpoint of a C4 home, a parser that turns
//! each line the shim sends into observations for `c4-core::reduce`, a bounded inbox the app drains,
//! and the replay of the spool the shim writes while nothing listens.
//!
//! The server listens where [`c4_core::C4Home::endpoint`] says: on Windows a named pipe whose DACL
//! admits the current user alone, elsewhere a socket file with mode 0600 under the home, where every
//! connection's peer uid is checked against the owner of the home. One line is one payload. A line
//! the parser cannot use is counted and logged by size and reason, never by content, because hook
//! payloads carry whatever the session typed.
//!
//! Nothing here blocks a producer: the inbox drops its oldest item when full and counts the drop, a
//! connection that stays silent is closed after [`ServerConfig::idle_timeout`], and one that sends a
//! line over [`ServerConfig::max_line_bytes`] is dropped.
//!
//! Replay is at least once. A C4 that dies between reading a spool file and deleting it reads the
//! file again at the next start, so the same lines reach the inbox twice with the same stamps; the
//! reducer treats a fact stamped earlier than what it already knows as history.
//!
//! Not covered by a test, on either platform: that a connection from another user is refused. It
//! would need a second account on the machine.

#[cfg(windows)]
pub mod acl;
pub mod inbox;
pub mod parse;
pub mod probe;
pub mod server;
pub mod spool;

pub use inbox::{Inbox, InboxStats};
pub use parse::{Ingested, Kind, ParseError, parse_line};
pub use probe::{Probe, probe};
pub use server::{Server, ServerConfig, ServerStats, StartError};
pub use spool::{ReplayReport, replay};
