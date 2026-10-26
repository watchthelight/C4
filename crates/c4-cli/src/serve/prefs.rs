// <wtl> project: c4 | file: crates/c4-cli/src/serve/prefs.rs | author: watchthelight </wtl>
//! What the window keeps between runs: `prefs.json` under the C4 home, one JSON object with a
//! `schema` number and one section per part of the window (`sidebar` today). The window owns the
//! shape of each section and reads it tolerantly; this side keeps the file whole. A section is
//! replaced as one piece under a mutex, the file is written through a sibling temporary file and
//! a rename, and a section this version does not know is carried along untouched, so what a newer
//! C4 saved survives a run of an older one. A file that cannot be read as a JSON object is moved
//! aside under a name with the time in it, the window starts from nothing, and `about` carries the
//! problem so the status strip can say so. The file lives under the C4 home, so a scratch home in
//! a test never touches the real one.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use c4_core::{C4Home, Timestamp};
use serde_json::{Map, Value};

/// The `schema` number written into the document.
pub const SCHEMA: u64 = 1;
/// The largest document read or written, so a runaway section cannot fill the disk or the window.
pub const MAX_BYTES: usize = 1 << 20;

/// Why a section could not be set.
#[derive(Debug, thiserror::Error)]
pub enum PrefsError {
    /// Sections are named by lowercase letters and underscores, and `schema` is not one.
    #[error("{0:?} is not a section name (lowercase letters and underscores, not \"schema\")")]
    BadSection(String),
    /// The document with the new section would be over [`MAX_BYTES`].
    #[error("the preferences would be {0} bytes, over the cap of {MAX_BYTES}")]
    TooLarge(usize),
    /// The file could not be written.
    #[error("could not write {path}: {source}")]
    Write {
        /// The file.
        path: PathBuf,
        /// The error.
        #[source]
        source: std::io::Error,
    },
}

/// The document and its file.
pub struct Prefs {
    path: PathBuf,
    doc: Mutex<Map<String, Value>>,
    problem: Option<String>,
}

impl Prefs {
    /// Read the file under `home`. A missing file is an empty document; a file that cannot be read
    /// as a JSON object is moved aside and the problem kept for [`Self::problem`].
    #[must_use]
    pub fn open(home: &C4Home) -> Self {
        let path = home.prefs_path();
        let (doc, problem) = match read(&path) {
            Ok(doc) => (doc, None),
            Err(problem) => {
                let aside = set_aside(&path);
                tracing::warn!(path = %path.display(), problem, aside = ?aside, "preferences unreadable");
                (Map::new(), Some(problem))
            }
        };
        Self {
            path,
            doc: Mutex::new(doc),
            problem,
        }
    }

    /// The file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What was wrong with the file at open, if anything.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// The whole document, with its `schema` number.
    #[must_use]
    pub fn document(&self) -> Value {
        let doc = self.doc.lock().unwrap_or_else(PoisonError::into_inner);
        Value::Object(with_schema(doc.clone()))
    }

    /// Replace one section and write the file. Returns the document as written; on an error the
    /// document and the file are as they were.
    pub fn set_section(&self, name: &str, value: Value) -> Result<Value, PrefsError> {
        if !section_name_ok(name) {
            return Err(PrefsError::BadSection(name.to_string()));
        }
        let mut doc = self.doc.lock().unwrap_or_else(PoisonError::into_inner);
        let mut next = with_schema(doc.clone());
        next.insert(name.to_string(), value);
        let mut bytes = serde_json::to_vec_pretty(&next).unwrap_or_default();
        bytes.push(b'\n');
        if bytes.len() > MAX_BYTES {
            return Err(PrefsError::TooLarge(bytes.len()));
        }
        write_atomic(&self.path, &bytes).map_err(|source| PrefsError::Write {
            path: self.path.clone(),
            source,
        })?;
        doc.clone_from(&next);
        Ok(Value::Object(next))
    }
}

/// Lowercase ASCII letters and underscores, at least one, and not the schema key.
fn section_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name != "schema"
        && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}

/// The map with `schema` set when it is missing. A number another version wrote stays.
fn with_schema(mut doc: Map<String, Value>) -> Map<String, Value> {
    if !doc.get("schema").is_some_and(Value::is_u64) {
        doc.insert("schema".to_string(), Value::from(SCHEMA));
    }
    doc
}

/// The file as a JSON object, or why it is not one. A missing file is an empty object.
fn read(path: &Path) -> Result<Map<String, Value>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(format!("could not read {}: {e}", path.display())),
    };
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "{} is {} bytes, over the cap of {MAX_BYTES}",
            path.display(),
            bytes.len()
        ));
    }
    match serde_json::from_slice::<Value>(&bytes) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(format!(
            "{} holds a JSON {} where an object was expected",
            path.display(),
            kind(&other)
        )),
        Err(e) => Err(format!("{} is not JSON: {e}", path.display())),
    }
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Move an unreadable file to `<name>.bad-<stamp>` beside it, so nothing is lost when the next
/// write replaces it. Returns the new name when the move worked.
fn set_aside(path: &Path) -> Option<PathBuf> {
    let stamp = Timestamp::now().to_file_stamp();
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".bad-{stamp}"));
    let aside = PathBuf::from(name);
    std::fs::rename(path, &aside).ok().map(|()| aside)
}

/// Write `bytes` to a sibling temporary file and rename it over `path`, creating the directory
/// (private to the user on Unix) when it is missing.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.is_dir()
    {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    let tmp = PathBuf::from(name);
    std::fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    // Windows refuses the rename for a moment while another process has the file open; a reader
    // (a second C4 on the same home) takes milliseconds
    let mut last = None;
    for attempt in 1..=3u64 {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(20 * attempt));
            }
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last.expect("three attempts were made"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn home(dir: &tempfile::TempDir) -> C4Home {
        C4Home::at(dir.path().join("c4"))
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn an_absent_file_is_an_empty_document_and_a_section_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        let prefs = Prefs::open(&home);
        assert_eq!(prefs.path(), home.prefs_path());
        assert!(prefs.problem().is_none());
        assert_eq!(prefs.document(), json!({ "schema": 1 }));
        assert!(!home.root().exists(), "opening creates nothing");

        let sidebar =
            json!({ "density": "compact", "group_by": "project", "pinned": ["a"], "width": 280 });
        let written = prefs.set_section("sidebar", sidebar.clone()).unwrap();
        assert_eq!(written, json!({ "schema": 1, "sidebar": sidebar }));
        assert_eq!(
            files(home.root()),
            vec!["prefs.json"],
            "no temporary file is left"
        );
        let text = std::fs::read_to_string(home.prefs_path()).unwrap();
        assert!(text.ends_with('\n'));
        assert!(text.starts_with("{\n  \"schema\": 1,"), "{text}");

        let again = Prefs::open(&home);
        assert_eq!(again.document(), written);
        let both = again
            .set_section("window", json!({ "detached": false }))
            .unwrap();
        assert_eq!(both["sidebar"], sidebar, "the other section is kept");
        assert_eq!(both["window"]["detached"], false);
        assert_eq!(Prefs::open(&home).document(), both);
    }

    #[test]
    fn a_section_another_version_wrote_and_a_newer_schema_number_survive() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        std::fs::create_dir_all(home.root()).unwrap();
        std::fs::write(
            home.prefs_path(),
            r#"{"schema": 7, "future": {"x": [1, 2]}, "sidebar": {"density": "expanded"}}"#,
        )
        .unwrap();
        let prefs = Prefs::open(&home);
        assert!(prefs.problem().is_none());
        let doc = prefs
            .set_section("sidebar", json!({ "density": "compact" }))
            .unwrap();
        assert_eq!(doc["schema"], 7);
        assert_eq!(doc["future"], json!({ "x": [1, 2] }));
        assert_eq!(doc["sidebar"], json!({ "density": "compact" }));
    }

    #[test]
    fn a_file_that_is_not_a_json_object_is_moved_aside() {
        for (bad, expect) in [
            ("[1, 2]", "holds a JSON array"),
            ("{\"schema\": 1", "is not JSON"),
            ("", "is not JSON"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let home = home(&dir);
            std::fs::create_dir_all(home.root()).unwrap();
            std::fs::write(home.prefs_path(), bad).unwrap();
            let prefs = Prefs::open(&home);
            let problem = prefs.problem().expect("a problem is reported");
            assert!(problem.contains(expect), "{problem}");
            assert_eq!(prefs.document(), json!({ "schema": 1 }));
            let names = files(home.root());
            assert_eq!(names.len(), 1, "{names:?}");
            assert!(names[0].starts_with("prefs.json.bad-"), "{names:?}");
            assert_eq!(
                std::fs::read_to_string(home.root().join(&names[0])).unwrap(),
                bad,
                "the bytes are kept"
            );
            prefs.set_section("sidebar", json!({})).unwrap();
            assert_eq!(files(home.root()).len(), 2, "the file aside stays");
        }
    }

    #[test]
    fn a_section_over_the_cap_is_refused_and_the_file_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let home = home(&dir);
        let prefs = Prefs::open(&home);
        let before = prefs
            .set_section("sidebar", json!({ "width": 320 }))
            .unwrap();
        let bytes = std::fs::read(home.prefs_path()).unwrap();
        let big = "x".repeat(MAX_BYTES);
        let err = prefs
            .set_section("sidebar", json!({ "note": big }))
            .unwrap_err();
        assert!(
            matches!(err, PrefsError::TooLarge(n) if n > MAX_BYTES),
            "{err}"
        );
        assert_eq!(prefs.document(), before);
        assert_eq!(std::fs::read(home.prefs_path()).unwrap(), bytes);
        assert_eq!(files(home.root()), vec!["prefs.json"]);
    }

    #[test]
    fn section_names_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = Prefs::open(&home(&dir));
        for name in ["", "schema", "Sidebar", "side bar", "side-bar", "sidebar2"] {
            let err = prefs.set_section(name, json!({})).unwrap_err();
            assert!(matches!(err, PrefsError::BadSection(_)), "{name:?}: {err}");
        }
        assert!(prefs.set_section("side_bar", json!(null)).is_ok());
        assert!(!dir.path().join("c4").join("prefs.json.tmp").exists());
    }
}
