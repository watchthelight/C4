// <wtl> project: c4 | file: crates/c4-ingest/src/probe.rs | author: watchthelight </wtl>
//! Is anything listening on a home's endpoint? For `c4 doctor`, with the standard library only, so
//! the answer does not need a runtime.

use c4_core::C4Home;

/// The answer.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Probe {
    /// Something accepted a connection on the endpoint.
    Listening,
    /// Nothing listens; the shim would spool.
    Free,
    /// Could not tell; the text says why.
    Unknown(String),
}

/// Try to connect to the endpoint of `home` and close at once. The server counts the connection
/// and reads nothing from it.
#[must_use]
pub fn probe(home: &C4Home) -> Probe {
    platform_probe(home)
}

#[cfg(windows)]
fn platform_probe(home: &C4Home) -> Probe {
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_PIPE_BUSY: i32 = 231;
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(home.pipe_name())
    {
        Ok(_) => Probe::Listening,
        Err(e) => match e.raw_os_error() {
            Some(ERROR_FILE_NOT_FOUND) => Probe::Free,
            // Every instance is taken right now, which only a listening server can cause.
            Some(ERROR_PIPE_BUSY) => Probe::Listening,
            Some(ERROR_ACCESS_DENIED) => {
                Probe::Unknown("the pipe exists but belongs to another user".to_string())
            }
            _ => Probe::Unknown(e.to_string()),
        },
    }
}

#[cfg(unix)]
fn platform_probe(home: &C4Home) -> Probe {
    let path = home.socket_path();
    if !path.exists() {
        return Probe::Free;
    }
    match std::os::unix::net::UnixStream::connect(&path) {
        Ok(_) => Probe::Listening,
        // A socket file nobody answers on: left behind by a C4 that did not get to remove it.
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            Probe::Free
        }
        Err(e) => Probe::Unknown(e.to_string()),
    }
}

#[cfg(not(any(windows, unix)))]
fn platform_probe(_home: &C4Home) -> Probe {
    Probe::Unknown("no local socket on this platform".to_string())
}
