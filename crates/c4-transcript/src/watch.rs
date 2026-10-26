// <wtl> project: c4 | file: crates/c4-transcript/src/watch.rs | author: watchthelight </wtl>
//! A watcher over the projects directory that reports which transcript or agent file changed, so
//! followers poll on demand. The watcher is a hint, never the truth: a missed event costs a poll
//! on the next timer tick, nothing more.

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// What happened to a file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeKind {
    /// Created or written.
    Written,
    /// Removed or renamed away.
    Removed,
}

/// One change.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Change {
    /// The file.
    pub path: PathBuf,
    /// What happened.
    pub kind: ChangeKind,
}

/// True for the files the follower cares about: `.jsonl` transcripts and `.meta.json` files.
#[must_use]
pub fn is_transcript_path(path: &Path) -> bool {
    let has_ext =
        |p: &Path, want: &str| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(want));
    if has_ext(path, "jsonl") {
        return true;
    }
    has_ext(path, "json")
        && path
            .file_stem()
            .is_some_and(|stem| has_ext(Path::new(stem), "meta"))
}

/// Watches a projects directory recursively.
pub struct TranscriptWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<Change>,
}

impl std::fmt::Debug for TranscriptWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranscriptWatcher").finish_non_exhaustive()
    }
}

fn forward(tx: &Sender<Change>, event: Event) {
    let kind = match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) => ChangeKind::Written,
        EventKind::Remove(_) => ChangeKind::Removed,
        _ => return,
    };
    for path in event.paths {
        if is_transcript_path(&path) {
            let _ = tx.send(Change { path, kind });
        }
    }
}

impl TranscriptWatcher {
    /// Watch `projects_dir` and everything under it. The directory must exist.
    pub fn new(projects_dir: &Path) -> notify::Result<Self> {
        let (tx, rx) = channel();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<Event>| match res {
                Ok(event) => forward(&tx, event),
                Err(e) => tracing::warn!(error = %e, "transcript watcher"),
            })?;
        watcher.watch(projects_dir, RecursiveMode::Recursive)?;
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// The changes queued so far, without waiting.
    pub fn drain(&self) -> Vec<Change> {
        self.rx.try_iter().collect()
    }

    /// Wait up to `timeout` for the next change.
    pub fn next_timeout(&self, timeout: std::time::Duration) -> Option<Change> {
        self.rx.recv_timeout(timeout).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::io::Write;
    use std::time::{Duration, Instant};

    #[test]
    fn transcript_paths() {
        assert!(is_transcript_path(Path::new("/p/s.jsonl")));
        assert!(is_transcript_path(Path::new(
            "/p/s/subagents/agent-x.meta.json"
        )));
        assert!(!is_transcript_path(Path::new("/p/s/subagents/notes.txt")));
        assert!(!is_transcript_path(Path::new("/p/s/workflows/wf_1.json")));
    }

    #[test]
    fn reports_a_written_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("-home-example-proj");
        std::fs::create_dir_all(&project).unwrap();
        let watcher = TranscriptWatcher::new(dir.path()).unwrap();
        // Give the watcher a moment to arm on platforms that set up asynchronously.
        std::thread::sleep(Duration::from_millis(200));
        let path = project.join("abc.jsonl");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "{{}}").unwrap();
        f.flush().unwrap();
        drop(f);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = false;
        while Instant::now() < deadline {
            if let Some(change) = watcher.next_timeout(Duration::from_millis(500))
                && change.path.file_name().and_then(OsStr::to_str) == Some("abc.jsonl")
                && change.kind == ChangeKind::Written
            {
                seen = true;
                break;
            }
        }
        assert!(seen, "no write event for the new transcript within 10 s");
        assert!(watcher.drain().iter().all(|c| is_transcript_path(&c.path)));
    }
}
