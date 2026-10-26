// <wtl> project: c4 | file: crates/c4-ingest/src/spool.rs | author: watchthelight </wtl>
//! Replay of `<home>/spool/<date>.jsonl`, the files the shim appends to while nothing listens.
//!
//! Each file is claimed by renaming it to `<name>.replaying`, so a shim that spools its next line
//! starts a fresh file and the one being read stops growing. A shim that opened the file before the
//! rename finishes its one write into the renamed file; the reader looks at the size once more after
//! reading and reads again when it moved. A file that cannot be renamed because a shim holds it open
//! (Windows refuses the rename then) is left for the next pass. A `.replaying` file found at startup
//! belongs to a C4 that died mid-replay and is read as it is.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use c4_core::C4Home;

use crate::inbox::Inbox;
use crate::parse::parse_line;

/// The extension a claimed spool file carries while it is read.
pub const REPLAYING: &str = "replaying";

/// What one pass over the spool did.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ReplayReport {
    /// Files read and removed.
    pub files: usize,
    /// Lines queued.
    pub lines: usize,
    /// Lines skipped because the parser refused them.
    pub bad_lines: usize,
    /// Files left in place, with the reason.
    pub problems: Vec<(PathBuf, String)>,
}

/// Read every spool file of `home` into `inbox`, oldest file first, and remove each one read.
pub fn replay(home: &C4Home, inbox: &Inbox) -> ReplayReport {
    let mut report = ReplayReport::default();
    let Ok(entries) = std::fs::read_dir(home.spool_dir()) else {
        return report;
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_spool_file(path))
        .collect();
    paths.sort();
    for path in paths {
        match replay_file(&path, inbox, &mut report) {
            Ok(()) => report.files += 1,
            Err(e) => report.problems.push((path, e.to_string())),
        }
    }
    report
}

/// `<name>.jsonl` or `<name>.jsonl.replaying`.
fn is_spool_file(path: &Path) -> bool {
    match path.extension().and_then(OsStr::to_str) {
        Some(ext) if ext.eq_ignore_ascii_case("jsonl") => true,
        Some(ext) if ext.eq_ignore_ascii_case(REPLAYING) => path
            .file_stem()
            .map(Path::new)
            .and_then(Path::extension)
            .and_then(OsStr::to_str)
            .is_some_and(|inner| inner.eq_ignore_ascii_case("jsonl")),
        _ => false,
    }
}

fn replay_file(path: &Path, inbox: &Inbox, report: &mut ReplayReport) -> io::Result<()> {
    let claimed = if path.extension().is_some_and(|ext| ext == REPLAYING) {
        path.to_path_buf()
    } else {
        let mut name = path
            .file_name()
            .map(OsStr::to_os_string)
            .unwrap_or_default();
        name.push(".");
        name.push(REPLAYING);
        let target = path.with_file_name(name);
        std::fs::rename(path, &target)?;
        target
    };
    let mut text = std::fs::read(&claimed)?;
    for _ in 0..3 {
        let on_disk = std::fs::metadata(&claimed)?.len();
        if on_disk <= u64::try_from(text.len()).unwrap_or(u64::MAX) {
            break;
        }
        text = std::fs::read(&claimed)?;
    }
    for raw in text.split(|b| *b == b'\n') {
        let line = String::from_utf8_lossy(raw);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match parse_line(line) {
            Ok(item) => {
                inbox.push(item);
                report.lines += 1;
            }
            Err(_) => report.bad_lines += 1,
        }
    }
    if let Err(e) = std::fs::remove_file(&claimed) {
        // Keep the lines from being replayed twice even if the file cannot go.
        let _ = std::fs::File::create(&claimed);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"kind":"hook","recv_at":7,"pid":1,"payload":{"session_id":"s1","hook_event_name":"Stop","background_tasks":[],"session_crons":[]}}"#;

    #[test]
    fn files_are_read_in_name_order_and_removed() {
        let dir = tempfile::Builder::new()
            .prefix("c4-ingest-")
            .tempdir()
            .unwrap();
        let home = C4Home::at(dir.path().join("c4"));
        let spool = home.spool_dir();
        std::fs::create_dir_all(&spool).unwrap();
        std::fs::write(spool.join("2026-10-26.jsonl"), format!("{GOOD}\n{GOOD}\n")).unwrap();
        std::fs::write(
            spool.join("2026-10-25.jsonl.replaying"),
            format!("{GOOD}\nnot json\n\n"),
        )
        .unwrap();
        std::fs::write(spool.join("notes.txt"), "not a spool file").unwrap();
        let inbox = Inbox::new(16);
        let report = replay(&home, &inbox);
        assert_eq!(report.files, 2);
        assert_eq!(report.lines, 3);
        assert_eq!(report.bad_lines, 1);
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(inbox.len(), 3);
        let left: Vec<String> = std::fs::read_dir(&spool)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["notes.txt".to_string()]);
    }

    #[test]
    fn no_spool_directory_is_not_a_problem() {
        let dir = tempfile::Builder::new()
            .prefix("c4-ingest-")
            .tempdir()
            .unwrap();
        let home = C4Home::at(dir.path().join("c4"));
        let inbox = Inbox::new(16);
        assert_eq!(replay(&home, &inbox), ReplayReport::default());
    }
}
