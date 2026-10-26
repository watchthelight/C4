// <wtl> project: c4 | file: crates/c4-transcript/src/index.rs | author: watchthelight </wtl>
//! Which transcripts exist, from file metadata alone. A transcript file is history, not a live
//! process: the index says a session ran, never that it runs. Discovery decides liveness.

use c4_core::{SessionId, Timestamp};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// One main transcript.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexEntry {
    /// The session id (the file stem).
    pub session: SessionId,
    /// The transcript.
    pub path: PathBuf,
    /// The project directory name (the encoded cwd).
    pub project: String,
    /// The file size.
    pub size: u64,
    /// When it was last written.
    pub modified: Option<Timestamp>,
    /// True when a `<session>/` directory exists beside it (subagents or workflows ran).
    pub has_dir: bool,
}

/// The index of a projects directory.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ProjectIndex {
    /// Every transcript, newest first.
    pub entries: Vec<IndexEntry>,
    /// Project directories walked.
    pub dir_count: usize,
}

impl ProjectIndex {
    /// The entry for a session, when the index has it.
    #[must_use]
    pub fn get(&self, session: &SessionId) -> Option<&IndexEntry> {
        self.entries.iter().find(|e| &e.session == session)
    }

    /// Transcripts written within `window_ms` of `now`, either side.
    pub fn recent(&self, now: Timestamp, window_ms: i64) -> impl Iterator<Item = &IndexEntry> {
        self.entries.iter().filter(move |e| {
            e.modified
                .is_some_and(|m| now.millis_since(m).abs() <= window_ms)
        })
    }
}

/// Walk `projects_dir` one level deep: every `<project>/<session>.jsonl`. Only `stat` is called on
/// each file; nothing is opened.
#[must_use]
pub fn index_projects(projects_dir: &Path) -> ProjectIndex {
    let mut index = ProjectIndex::default();
    let Ok(projects) = std::fs::read_dir(projects_dir) else {
        return index;
    };
    for project in projects.flatten() {
        let project_path = project.path();
        if !project_path.is_dir() {
            continue;
        }
        index.dir_count += 1;
        let Some(project_name) = project_path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(&project_path) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            let is_jsonl = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"));
            let Some(stem) = path
                .file_stem()
                .and_then(OsStr::to_str)
                .filter(|_| is_jsonl)
            else {
                continue;
            };
            let Ok(meta) = file.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            index.entries.push(IndexEntry {
                session: SessionId::new(stem),
                project: project_name.to_string(),
                size: meta.len(),
                modified: meta.modified().ok().and_then(Timestamp::from_system_time),
                has_dir: path.with_extension("").is_dir(),
                path,
            });
        }
    }
    index.entries.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.session.as_str().cmp(b.session.as_str()))
    });
    index
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn indexes_the_fixture_projects() {
        let index = index_projects(&fixtures());
        assert_eq!(index.dir_count, 4);
        assert_eq!(index.entries.len(), 4);
        let alpha = index
            .get(&SessionId::new("0a1b2c3d-0001-4000-8000-000000000001"))
            .unwrap();
        assert!(alpha.has_dir);
        assert_eq!(alpha.project, "-home-example-proj-alpha");
        assert_eq!(alpha.size, 6939);
        let delta = index
            .get(&SessionId::new("0a1b2c3d-0001-4000-8000-000000000004"))
            .unwrap();
        assert_eq!(delta.size, 0);
        assert!(!delta.has_dir);
        let ten_years = 1000 * 60 * 60 * 24 * 3650;
        assert_eq!(index.recent(Timestamp::now(), ten_years).count(), 4);
        assert_eq!(index.recent(Timestamp::from_millis(0), 1).count(), 0);
    }

    #[test]
    fn missing_directory_is_empty() {
        assert_eq!(
            index_projects(Path::new("/no/such/projects")),
            ProjectIndex::default()
        );
    }
}
