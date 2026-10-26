// <wtl> project: c4 | file: crates/c4-transcript/src/tailer.rs | author: watchthelight </wtl>
//! Reads a JSONL file from where the last read stopped. Claude Code appends whole lines, but a
//! read can land in the middle of one, so only lines that end in a newline count; the partial tail
//! stays in the file for the next poll and the offset never moves past it.

use crate::record::Record;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The most bytes one poll reads. A 143 MB file is read in slices so that one poll never stalls the
/// caller; [`Poll::more`] says when to poll again at once.
pub const MAX_CHUNK: u64 = 8 * 1024 * 1024;

/// Where a tailer stopped. Persisted by the caller so a restart never rereads a file.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Cursor {
    /// The file.
    pub path: PathBuf,
    /// The byte after the last complete line consumed.
    pub offset: u64,
    /// The file size at the last poll.
    pub size: u64,
    /// Complete lines consumed so far.
    pub lines: u64,
    /// Lines that were not JSON objects.
    pub skipped: u64,
    /// The `uuid` of the last content record consumed.
    pub last_uuid: Option<String>,
}

impl Cursor {
    /// A cursor at the start of `path`.
    #[must_use]
    pub fn start(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            size: 0,
            lines: 0,
            skipped: 0,
            last_uuid: None,
        }
    }
}

/// Why a poll could not read.
#[derive(thiserror::Error, Debug)]
pub enum TailError {
    /// The file is gone (Claude Code's cleanup removes transcripts after `cleanupPeriodDays`).
    #[error("transcript vanished: {0}")]
    Vanished(PathBuf),
    /// Any other I/O failure.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        #[source]
        source: std::io::Error,
    },
}

/// What one poll produced.
#[derive(Debug, Default)]
pub struct Poll {
    /// The records of the complete lines read, in file order.
    pub records: Vec<Record>,
    /// Lines in this poll that did not parse.
    pub skipped: u64,
    /// True when the file was smaller than the cursor and reading restarted at byte 0.
    pub truncated: bool,
    /// True when the chunk limit stopped the read and the file has more complete lines.
    pub more: bool,
    /// Bytes of a partial last line left for next time.
    pub partial: u64,
}

/// A byte-offset tailer for one file.
#[derive(Clone, Debug)]
pub struct Tailer {
    cursor: Cursor,
}

impl Tailer {
    /// A tailer at the start of `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            cursor: Cursor::start(path),
        }
    }

    /// A tailer that resumes from a saved cursor.
    #[must_use]
    pub fn resume(cursor: Cursor) -> Self {
        Self { cursor }
    }

    /// Where it is.
    #[must_use]
    pub fn cursor(&self) -> &Cursor {
        &self.cursor
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.cursor.path
    }

    /// Read every complete line written since the last poll, up to [`MAX_CHUNK`] bytes.
    pub fn poll(&mut self) -> Result<Poll, TailError> {
        let path = self.cursor.path.clone();
        let io = |source: std::io::Error| match source.kind() {
            std::io::ErrorKind::NotFound => TailError::Vanished(path.clone()),
            _ => TailError::Io {
                path: path.clone(),
                source,
            },
        };
        let mut file = File::open(&path).map_err(io)?;
        let size = file.metadata().map_err(io)?.len();
        let mut out = Poll::default();
        if size < self.cursor.offset {
            tracing::info!(path = %path.display(), size, offset = self.cursor.offset, "transcript shrank, reading from the start");
            self.cursor.offset = 0;
            self.cursor.lines = 0;
            self.cursor.skipped = 0;
            self.cursor.last_uuid = None;
            out.truncated = true;
        }
        self.cursor.size = size;
        let available = size - self.cursor.offset;
        if available == 0 {
            return Ok(out);
        }
        let want = available.min(MAX_CHUNK);
        file.seek(SeekFrom::Start(self.cursor.offset)).map_err(io)?;
        let mut buf = Vec::with_capacity(want as usize);
        file.take(want).read_to_end(&mut buf).map_err(io)?;

        // Only complete lines count. Everything after the last newline waits for the next poll.
        let Some(last_newline) = buf.iter().rposition(|&b| b == b'\n') else {
            out.partial = buf.len() as u64;
            return Ok(out);
        };
        let complete = last_newline + 1;
        out.partial = (buf.len() - complete) as u64;
        out.more = (self.cursor.offset + complete as u64) < size && want == MAX_CHUNK;
        let text = String::from_utf8_lossy(&buf[..complete]);
        for line in text.split_inclusive('\n') {
            let trimmed = line.trim_end_matches(['\n', '\r']);
            self.cursor.lines += 1;
            if trimmed.trim().is_empty() {
                continue;
            }
            match Record::parse(trimmed) {
                Ok(record) => {
                    if record.is_content() && record.uuid.is_some() {
                        self.cursor.last_uuid.clone_from(&record.uuid);
                    }
                    out.records.push(record);
                }
                Err(e) => {
                    tracing::debug!(path = %path.display(), line = self.cursor.lines, error = %e, "unparseable transcript line");
                    out.skipped += 1;
                    self.cursor.skipped += 1;
                }
            }
        }
        self.cursor.offset += complete as u64;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the workspace root")
            .join("fixtures")
            .join("home")
            .join("projects")
    }

    #[test]
    fn reads_a_whole_fixture_in_one_poll() {
        let path = fixtures()
            .join("-home-example-proj-alpha")
            .join("0a1b2c3d-0001-4000-8000-000000000001.jsonl");
        let mut t = Tailer::new(&path);
        let poll = t.poll().unwrap();
        assert_eq!(poll.records.len(), 15);
        assert_eq!(poll.skipped, 0);
        assert_eq!(poll.partial, 0);
        assert!(!poll.more && !poll.truncated);
        assert_eq!(t.cursor().offset, t.cursor().size);
        assert_eq!(t.cursor().lines, 15);
        assert_eq!(
            t.cursor().last_uuid.as_deref(),
            Some("f000000c-0000-4000-8000-00000000000c")
        );
        let again = t.poll().unwrap();
        assert!(again.records.is_empty(), "nothing new, nothing reread");
    }

    #[test]
    fn leaves_a_partial_last_line_alone() {
        let path = fixtures()
            .join("-home-example-proj-beta")
            .join("0a1b2c3d-0001-4000-8000-000000000002.jsonl");
        let mut t = Tailer::new(&path);
        let poll = t.poll().unwrap();
        assert_eq!(poll.records.len(), 3);
        assert_eq!(poll.partial, 239);
        assert_eq!(t.cursor().offset, t.cursor().size - 239);
        let again = t.poll().unwrap();
        assert_eq!(again.records, Vec::new());
        assert_eq!(again.partial, 239);
    }

    #[test]
    fn empty_and_missing_files() {
        let path = fixtures()
            .join("-home-example-proj-delta")
            .join("0a1b2c3d-0001-4000-8000-000000000004.jsonl");
        let mut t = Tailer::new(&path);
        let poll = t.poll().unwrap();
        assert_eq!(poll.records, Vec::new());
        let mut gone = Tailer::new(fixtures().join("nope").join("x.jsonl"));
        assert!(matches!(gone.poll(), Err(TailError::Vanished(_))));
    }

    #[test]
    fn follows_appends_and_a_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.jsonl");
        let mut f = File::create(&path).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","uuid":"u1","message":{{"content":"a"}}}}"#
        )
        .unwrap();
        write!(f, r#"{{"type":"assistant","uuid":"a1""#).unwrap();
        f.flush().unwrap();
        let mut t = Tailer::new(&path);
        let p = t.poll().unwrap();
        assert_eq!(p.records.len(), 1);
        assert!(p.partial > 0);
        assert_eq!(t.cursor().last_uuid.as_deref(), Some("u1"));

        // The assistant line is finished with a brace missing, so it never parses.
        writeln!(f, r#","message":{{"content":"b"}}"#).unwrap();
        writeln!(f, "not json").unwrap();
        writeln!(f).unwrap();
        writeln!(
            f,
            r#"{{"type":"system","uuid":"s1","subtype":"turn_duration"}}"#
        )
        .unwrap();
        f.flush().unwrap();
        let p = t.poll().unwrap();
        assert_eq!(p.records.len(), 1, "the broken assistant line is skipped");
        assert_eq!(p.skipped, 2, "the unbalanced line and the plain text");
        assert_eq!(t.cursor().last_uuid.as_deref(), Some("s1"));
        assert_eq!(t.cursor().lines, 5);

        // Resume from a saved cursor: nothing is reread.
        let saved = t.cursor().clone();
        let mut resumed = Tailer::resume(saved);
        assert_eq!(resumed.poll().unwrap().records, Vec::new());

        // A rewrite (smaller file) restarts from byte 0.
        drop(f);
        std::fs::write(&path, "{\"type\":\"user\",\"uuid\":\"u9\"}\n").unwrap();
        let p = t.poll().unwrap();
        assert!(p.truncated);
        assert_eq!(p.records.len(), 1);
        assert_eq!(t.cursor().last_uuid.as_deref(), Some("u9"));
        assert_eq!(t.cursor().lines, 1);
    }

    #[test]
    fn large_files_come_in_slices() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.jsonl");
        let line = format!(
            "{{\"type\":\"user\",\"uuid\":\"u\",\"pad\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let count = (MAX_CHUNK / line.len() as u64) + 10;
        let mut f = File::create(&path).unwrap();
        for _ in 0..count {
            f.write_all(line.as_bytes()).unwrap();
        }
        f.flush().unwrap();
        let mut t = Tailer::new(&path);
        let first = t.poll().unwrap();
        assert!(first.more);
        assert!(first.records.len() as u64 > count - 20);
        let second = t.poll().unwrap();
        assert!(!second.more);
        assert_eq!(
            first.records.len() as u64 + second.records.len() as u64,
            count
        );
        assert_eq!(t.cursor().offset, t.cursor().size);
    }
}
