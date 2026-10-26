// <wtl> project: c4 | file: crates/c4-ingest/src/server.rs | author: watchthelight </wtl>
//! The listener on the ingest endpoint and what happens to each connection.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use c4_core::C4Home;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::watch;
use tokio::task::{JoinHandle, JoinSet};

use crate::inbox::Inbox;
use crate::parse::parse_line;
use crate::spool::{self, ReplayReport};

/// `ERROR_PIPE_LISTENING`. mio issues a read on a Windows pipe server as soon as it is registered,
/// before any client exists; on Windows 11 that read completes with this error and mio hands it to
/// the first read after `connect()`. The client's bytes are behind it, so the read is repeated.
const ERROR_PIPE_LISTENING: i32 = 536;

/// Knobs with defaults that suit the shim.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// A line longer than this drops its connection. The shim never sends more than 1 MiB.
    pub max_line_bytes: usize,
    /// A connection that sends nothing for this long is closed.
    pub idle_timeout: Duration,
    /// Replay the spool once right after listening, before accepting.
    pub replay_on_start: bool,
    /// Replay the spool again this often, for lines spooled while the server was away.
    pub replay_every: Option<Duration>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_line_bytes: 2 << 20,
            idle_timeout: Duration::from_secs(5),
            replay_on_start: true,
            replay_every: Some(Duration::from_secs(30)),
        }
    }
}

/// Counters since the server started.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ServerStats {
    /// Connections accepted.
    pub connections: u64,
    /// Connections refused because the peer was another user or had no credentials (Unix).
    pub refused: u64,
    /// Lines parsed and queued.
    pub lines: u64,
    /// Lines skipped.
    pub bad_lines: u64,
    /// Connections dropped for a line over the cap.
    pub oversize: u64,
    /// Connections closed for silence.
    pub idle_closed: u64,
    /// Spool files replayed and removed.
    pub replayed_files: u64,
    /// Lines queued from the spool.
    pub replayed_lines: u64,
    /// Spool lines skipped.
    pub replay_bad_lines: u64,
}

#[derive(Default)]
struct Counters {
    connections: AtomicU64,
    refused: AtomicU64,
    lines: AtomicU64,
    bad_lines: AtomicU64,
    oversize: AtomicU64,
    idle_closed: AtomicU64,
    replayed_files: AtomicU64,
    replayed_lines: AtomicU64,
    replay_bad_lines: AtomicU64,
}

impl Counters {
    fn snapshot(&self) -> ServerStats {
        ServerStats {
            connections: self.connections.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            lines: self.lines.load(Ordering::Relaxed),
            bad_lines: self.bad_lines.load(Ordering::Relaxed),
            oversize: self.oversize.load(Ordering::Relaxed),
            idle_closed: self.idle_closed.load(Ordering::Relaxed),
            replayed_files: self.replayed_files.load(Ordering::Relaxed),
            replayed_lines: self.replayed_lines.load(Ordering::Relaxed),
            replay_bad_lines: self.replay_bad_lines.load(Ordering::Relaxed),
        }
    }
}

/// Why the server could not start.
#[derive(thiserror::Error, Debug)]
pub enum StartError {
    /// Another process answers on the endpoint: a second C4 on the same home.
    #[error("another process listens on {endpoint}")]
    EndpointTaken {
        /// The pipe name or socket path.
        endpoint: String,
    },
    /// The home directory or the socket file could not be prepared.
    #[error("could not prepare {}: {source}", .path.display())]
    Home {
        /// What could not be prepared.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
    /// The endpoint could not be created.
    #[error("could not listen on {endpoint}: {source}")]
    Listen {
        /// The pipe name or socket path.
        endpoint: String,
        /// The cause.
        source: io::Error,
    },
    /// The pipe's DACL could not be built (Windows only).
    #[error("could not build the pipe's security descriptor: {0}")]
    Acl(#[source] io::Error),
}

#[derive(Clone)]
struct Ctx {
    inbox: Arc<Inbox>,
    counters: Arc<Counters>,
    max_line: usize,
    idle: Duration,
}

/// A running server. Dropping it tells its tasks to stop; [`Server::shutdown`] also waits for
/// them, gives open connections a moment to finish, and removes the socket file.
pub struct Server {
    home: C4Home,
    endpoint: String,
    config: ServerConfig,
    inbox: Arc<Inbox>,
    counters: Arc<Counters>,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Server {
    /// Listen on the home's endpoint, replay the spool when configured, and start accepting.
    pub async fn start(
        home: C4Home,
        inbox: Arc<Inbox>,
        config: ServerConfig,
    ) -> Result<Self, StartError> {
        let listener = platform::listen(&home)?;
        let endpoint = home.endpoint();
        let counters = Arc::new(Counters::default());
        if config.replay_on_start {
            replay_now(&home, &inbox, &counters).await;
        }
        let (stop, rx) = watch::channel(false);
        let ctx = Ctx {
            inbox: inbox.clone(),
            counters: counters.clone(),
            max_line: config.max_line_bytes,
            idle: config.idle_timeout,
        };
        let mut tasks = vec![tokio::spawn(platform::accept_loop(
            listener,
            ctx,
            rx.clone(),
        ))];
        if let Some(every) = config.replay_every {
            tasks.push(tokio::spawn(replay_loop(
                home.clone(),
                inbox.clone(),
                counters.clone(),
                every,
                rx,
            )));
        }
        tracing::info!(endpoint = %endpoint, "ingest server listening");
        Ok(Self {
            home,
            endpoint,
            config,
            inbox,
            counters,
            stop,
            tasks,
        })
    }

    /// The pipe name or socket path.
    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The home the server serves.
    #[must_use]
    pub fn home(&self) -> &C4Home {
        &self.home
    }

    /// The configuration it runs with.
    #[must_use]
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// The inbox lines land in.
    #[must_use]
    pub fn inbox(&self) -> &Arc<Inbox> {
        &self.inbox
    }

    /// The counters.
    #[must_use]
    pub fn stats(&self) -> ServerStats {
        self.counters.snapshot()
    }

    /// Replay the spool now, outside the schedule.
    pub async fn replay(&self) -> ReplayReport {
        replay_now(&self.home, &self.inbox, &self.counters).await
    }

    /// Stop accepting, wait for the tasks, and remove the socket file. Open connections get
    /// [`SHUTDOWN_GRACE`] to finish on their own, then they are dropped.
    pub async fn shutdown(mut self) {
        let _ = self.stop.send(true);
        for task in std::mem::take(&mut self.tasks) {
            let _ = task.await;
        }
        platform::cleanup(&self.home);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

/// How long [`Server::shutdown`] waits for open connections before dropping them.
pub const SHUTDOWN_GRACE: Duration = Duration::from_millis(250);

/// Wait for the connection tasks to end on their own, then abort what is left.
async fn drain(mut conns: JoinSet<()>) {
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, async {
        while conns.join_next().await.is_some() {}
    })
    .await;
    conns.shutdown().await;
}

async fn replay_loop(
    home: C4Home,
    inbox: Arc<Inbox>,
    counters: Arc<Counters>,
    every: Duration,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            () = tokio::time::sleep(every) => {
                replay_now(&home, &inbox, &counters).await;
            }
        }
    }
}

async fn replay_now(home: &C4Home, inbox: &Arc<Inbox>, counters: &Arc<Counters>) -> ReplayReport {
    let (home, inbox) = (home.clone(), inbox.clone());
    let report = match tokio::task::spawn_blocking(move || spool::replay(&home, &inbox)).await {
        Ok(report) => report,
        Err(e) => {
            tracing::warn!(error = %e, "spool replay did not finish");
            return ReplayReport::default();
        }
    };
    counters
        .replayed_files
        .fetch_add(report.files as u64, Ordering::Relaxed);
    counters
        .replayed_lines
        .fetch_add(report.lines as u64, Ordering::Relaxed);
    counters
        .replay_bad_lines
        .fetch_add(report.bad_lines as u64, Ordering::Relaxed);
    for (path, what) in &report.problems {
        tracing::warn!(path = %path.display(), what = %what, "spool file left for the next pass");
    }
    if report.files > 0 {
        tracing::debug!(
            files = report.files,
            lines = report.lines,
            bad_lines = report.bad_lines,
            "spool replayed"
        );
    }
    report
}

/// Read lines from one connection until it closes, goes quiet or sends a line over the cap.
async fn serve<R: AsyncRead + Unpin + Send + 'static>(mut stream: R, ctx: Ctx) {
    ctx.counters.connections.fetch_add(1, Ordering::Relaxed);
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 16 * 1024];
    let mut stale_reads = 0u8;
    loop {
        let n = match tokio::time::timeout(ctx.idle, stream.read(&mut chunk)).await {
            Err(_elapsed) => {
                ctx.counters.idle_closed.fetch_add(1, Ordering::Relaxed);
                tracing::debug!("ingest connection closed for silence");
                return;
            }
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => n,
            Ok(Err(e)) if e.raw_os_error() == Some(ERROR_PIPE_LISTENING) && stale_reads < 3 => {
                stale_reads += 1;
                continue;
            }
            Ok(Err(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::BrokenPipe
                        | io::ErrorKind::ConnectionReset
                        | io::ErrorKind::UnexpectedEof
                ) =>
            {
                break;
            }
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "ingest connection read failed");
                break;
            }
        };
        pending.extend_from_slice(&chunk[..n]);
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            take_line(&line, &ctx);
        }
        if pending.len() > ctx.max_line {
            ctx.counters.oversize.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                bytes = pending.len(),
                "ingest connection dropped: a line over the cap"
            );
            return;
        }
    }
    // A last line the client did not terminate still counts.
    if !pending.is_empty() {
        take_line(&pending, &ctx);
    }
}

fn take_line(bytes: &[u8], ctx: &Ctx) {
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    match parse_line(text) {
        Ok(item) => {
            ctx.counters.lines.fetch_add(1, Ordering::Relaxed);
            tracing::trace!(
                kind = item.kind.as_str(),
                hook_event = item.event.as_deref().unwrap_or(""),
                "ingest line"
            );
            ctx.inbox.push(item);
        }
        Err(e) => {
            ctx.counters.bad_lines.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(bytes = bytes.len(), error = %e, "ingest line skipped");
        }
    }
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::time::Duration;

    use c4_core::C4Home;
    use tokio::net::windows::named_pipe::{NamedPipeServer, PipeMode, ServerOptions};
    use tokio::sync::watch;
    use tokio::task::JoinSet;

    use super::{Ctx, StartError, drain, serve};
    use crate::acl::SecurityAttributes;

    /// `ERROR_ACCESS_DENIED`: what creating the first instance fails with when the name is taken.
    const ERROR_ACCESS_DENIED: i32 = 5;

    pub(super) struct Listener {
        name: String,
        attrs: SecurityAttributes,
        next: Option<NamedPipeServer>,
    }

    pub(super) fn listen(home: &C4Home) -> Result<Listener, StartError> {
        let name = home.pipe_name();
        let mut attrs = SecurityAttributes::current_user_only().map_err(StartError::Acl)?;
        let first = create(&name, &mut attrs, true).map_err(|source| {
            if source.raw_os_error() == Some(ERROR_ACCESS_DENIED) {
                StartError::EndpointTaken {
                    endpoint: name.clone(),
                }
            } else {
                StartError::Listen {
                    endpoint: name.clone(),
                    source,
                }
            }
        })?;
        tracing::debug!(dacl = attrs.sddl(), "ingest pipe created");
        Ok(Listener {
            name,
            attrs,
            next: Some(first),
        })
    }

    /// One pipe instance with the current-user DACL. The first instance claims the name.
    #[allow(unsafe_code)]
    fn create(
        name: &str,
        attrs: &mut SecurityAttributes,
        first: bool,
    ) -> io::Result<NamedPipeServer> {
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .pipe_mode(PipeMode::Byte)
            .access_inbound(true)
            .access_outbound(true);
        // SAFETY: `attrs.as_ptr()` points at a SECURITY_ATTRIBUTES whose descriptor `attrs` owns
        // and frees only when dropped, after this call; CreateNamedPipeW reads it during the call.
        unsafe { options.create_with_security_attributes_raw(name, attrs.as_ptr()) }
    }

    pub(super) async fn accept_loop(
        mut listener: Listener,
        ctx: Ctx,
        mut stop: watch::Receiver<bool>,
    ) {
        let mut conns = JoinSet::new();
        loop {
            while conns.try_join_next().is_some() {}
            let server = if let Some(server) = listener.next.take() {
                server
            } else {
                match create(&listener.name, &mut listener.attrs, false) {
                    Ok(server) => server,
                    Err(e) => {
                        tracing::warn!(error = %e, "could not create the next pipe instance");
                        tokio::select! {
                            _ = stop.changed() => break,
                            () = tokio::time::sleep(Duration::from_millis(250)) => continue,
                        }
                    }
                }
            };
            tokio::select! {
                _ = stop.changed() => break,
                connected = server.connect() => {
                    if let Err(e) = connected {
                        tracing::debug!(error = %e, "pipe connect failed");
                        continue;
                    }
                }
            }
            // The next client must find an instance to open while this one is served.
            listener.next = create(&listener.name, &mut listener.attrs, false).ok();
            conns.spawn(serve(server, ctx.clone()));
        }
        drop(listener);
        drain(conns).await;
    }

    pub(super) fn cleanup(_home: &C4Home) {}
}

#[cfg(unix)]
mod platform {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    use c4_core::C4Home;
    use tokio::net::UnixListener;
    use tokio::sync::watch;
    use tokio::task::JoinSet;

    use super::{Ctx, StartError, drain, serve};

    pub(super) struct Listener {
        socket: UnixListener,
        path: PathBuf,
        uid: u32,
    }

    pub(super) fn listen(home: &C4Home) -> Result<Listener, StartError> {
        let root = home.root().to_path_buf();
        let prepare = |source| StartError::Home {
            path: root.clone(),
            source,
        };
        std::fs::create_dir_all(&root).map_err(prepare)?;
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).map_err(prepare)?;
        let uid = std::fs::metadata(&root).map_err(prepare)?.uid();
        let path = home.socket_path();
        let endpoint = path.to_string_lossy().into_owned();
        if path.exists() {
            if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                return Err(StartError::EndpointTaken { endpoint });
            }
            // A socket file nobody answers on: a C4 that did not get to remove it.
            std::fs::remove_file(&path).map_err(|source| StartError::Home {
                path: path.clone(),
                source,
            })?;
        }
        let socket = UnixListener::bind(&path).map_err(|source| StartError::Listen {
            endpoint: endpoint.clone(),
            source,
        })?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(
            |source| StartError::Home {
                path: path.clone(),
                source,
            },
        )?;
        Ok(Listener { socket, path, uid })
    }

    pub(super) async fn accept_loop(listener: Listener, ctx: Ctx, mut stop: watch::Receiver<bool>) {
        let mut conns = JoinSet::new();
        loop {
            while conns.try_join_next().is_some() {}
            tokio::select! {
                _ = stop.changed() => break,
                accepted = listener.socket.accept() => match accepted {
                    Ok((stream, _)) => match stream.peer_cred() {
                        Ok(cred) if cred.uid() == listener.uid => {
                            conns.spawn(serve(stream, ctx.clone()));
                        }
                        Ok(cred) => {
                            ctx.counters.refused.fetch_add(1, Ordering::Relaxed);
                            tracing::warn!(uid = cred.uid(), "ingest connection from another user refused");
                        }
                        Err(e) => {
                            ctx.counters.refused.fetch_add(1, Ordering::Relaxed);
                            tracing::warn!(error = %e, "ingest connection refused: no peer credentials");
                        }
                    },
                    Err(e) => {
                        tracing::debug!(error = %e, "accept failed");
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                },
            }
        }
        drop(listener.socket);
        let _ = std::fs::remove_file(&listener.path);
        drain(conns).await;
    }

    pub(super) fn cleanup(home: &C4Home) {
        let _ = std::fs::remove_file(home.socket_path());
    }
}

#[cfg(not(any(windows, unix)))]
mod platform {
    use c4_core::C4Home;
    use tokio::sync::watch;

    use super::{Ctx, StartError};

    pub(super) struct Listener;

    pub(super) fn listen(home: &C4Home) -> Result<Listener, StartError> {
        Err(StartError::Listen {
            endpoint: home.endpoint(),
            source: std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "no local socket on this platform",
            ),
        })
    }

    pub(super) async fn accept_loop(_: Listener, _: Ctx, _: watch::Receiver<bool>) {}

    pub(super) fn cleanup(_: &C4Home) {}
}
