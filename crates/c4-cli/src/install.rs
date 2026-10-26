// <wtl> project: c4 | file: crates/c4-cli/src/install.rs | author: watchthelight </wtl>
//! `c4 install` and `c4 uninstall`: the one place C4 writes to a Claude home.
//!
//! Install copies the shim into the C4 home, wraps `statusLine.command` in `settings.json` so the
//! payload reaches C4 before the status line runs, adds one asynchronous hook group per event C4
//! listens to (and a synchronous one for `SessionEnd`), and records what it did in
//! `<c4 home>/install-manifest.json`. Uninstall puts the file back as it was when nothing else has
//! written it since C4 did, and otherwise removes C4's entries alone.
//!
//! Every write of `settings.json` is preceded by a backup beside it, goes through a temporary file
//! and a rename, and keeps the file's key order, indentation, line endings, byte order mark and
//! permissions. A file that does not parse stops the command before anything is written. A second
//! install changes nothing. Neither command runs the shim or any other program.

use crate::args::{InstallArgs, UninstallArgs};
use anyhow::{Context, bail};
use c4_core::Timestamp;
use c4_core::home::{C4Home, user_home};
use c4_core::redact::redact;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The events C4 listens to, each hook's timeout in seconds and whether it runs asynchronously.
/// Every one is a command hook that prints nothing and exits 0, so none can block a session. The
/// one synchronous hook, `SessionEnd`, stays inside the 1.5 s Claude Code gives that event.
pub const HOOK_EVENTS: [(&str, u64, bool); 17] = [
    ("SessionStart", 5, true),
    ("UserPromptSubmit", 5, true),
    ("PreToolUse", 5, true),
    ("PostToolUse", 5, true),
    ("PostToolUseFailure", 5, true),
    ("Notification", 5, true),
    ("SubagentStart", 5, true),
    ("SubagentStop", 5, true),
    ("TaskCreated", 5, true),
    ("TaskCompleted", 5, true),
    ("Stop", 5, true),
    ("StopFailure", 5, true),
    ("PreCompact", 5, true),
    ("PostCompact", 5, true),
    ("CwdChanged", 5, true),
    ("ConfigChange", 5, true),
    ("SessionEnd", 1, false),
];

/// The version of the manifest's shape.
pub const MANIFEST_SCHEMA: u32 = 1;

/// The shim's file name on this platform.
#[must_use]
pub fn shim_file_name() -> String {
    format!("c4-shim{}", std::env::consts::EXE_SUFFIX)
}

/// How `settings.json` names the shim. For the default C4 home it is `~/.c4/bin/c4-shim` (with
/// `.exe` on Windows), which bash expands wherever the settings file is read. For any other home it
/// is the absolute path, with a `C4_HOME=<home>` assignment ahead of it so the shim resolves the same
/// home the app does.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ShimCommand {
    prefix: String,
    program: String,
}

impl ShimCommand {
    /// The command text for `c4_home`, given the user's home directory.
    pub fn for_home(c4_home: &C4Home, user_home: &Path) -> anyhow::Result<Self> {
        let exe = shim_file_name();
        if is_default_home(c4_home.root(), user_home) {
            return Ok(Self {
                prefix: String::new(),
                program: format!("~/.c4/bin/{exe}"),
            });
        }
        let root = std::path::absolute(c4_home.root()).with_context(|| {
            format!(
                "the C4 home {} has no absolute form",
                c4_home.root().display()
            )
        })?;
        let home_text = shell_word(&root)?;
        let program = shell_word(&root.join("bin").join(exe))?;
        Ok(Self {
            prefix: format!("C4_HOME={home_text} "),
            program,
        })
    }

    /// The hook command: `<shim> hook`.
    #[must_use]
    pub fn hook(&self) -> String {
        format!("{}{} hook", self.prefix, self.program)
    }

    /// What the wrapper puts in front of the original status line command.
    #[must_use]
    pub fn statusline_segment(&self) -> String {
        format!("{}{} statusline -- ", self.prefix, self.program)
    }

    /// The status line command with the shim ahead of it, inserted after any leading `NAME=value`
    /// words so the environment the original command relies on still reaches it. `None` when the
    /// command has no program word.
    #[must_use]
    pub fn wrap(&self, command: &str) -> Option<String> {
        let at = program_start(command)?;
        Some(format!(
            "{}{}{}",
            &command[..at],
            self.statusline_segment(),
            &command[at..]
        ))
    }

    /// The inverse of [`Self::wrap`]: the command without this shim's segment, or `None` when the
    /// segment is not in it.
    #[must_use]
    pub fn unwrap(&self, command: &str) -> Option<String> {
        let segment = self.statusline_segment();
        command
            .find(&segment)
            .map(|at| format!("{}{}", &command[..at], &command[at + segment.len()..]))
    }
}

fn is_default_home(root: &Path, user_home: &Path) -> bool {
    let default = user_home.join(".c4");
    if cfg!(windows) {
        c4_core::home::home_key(root) == c4_core::home::home_key(&default)
    } else {
        let trimmed = |p: &Path| p.to_string_lossy().trim_end_matches('/').to_string();
        trimmed(root) == trimmed(&default)
    }
}

/// A path as a word bash reads back as that path: forward slashes on Windows, double quotes around
/// anything the shell would split or expand. Characters that stay special inside double quotes are
/// refused rather than escaped, since every hook runs its command through bash.
fn shell_word(path: &Path) -> anyhow::Result<String> {
    let Some(text) = path.to_str() else {
        bail!(
            "the path {} is not valid UTF-8; choose another C4 home",
            path.display()
        );
    };
    let text = if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.to_string()
    };
    if text
        .chars()
        .any(|c| matches!(c, '"' | '$' | '`' | '\\' | '\n' | '\r'))
    {
        bail!(
            "the path {text} holds a character bash would interpret (a double quote, a dollar sign, a backtick, a backslash or a line break); choose another C4 home"
        );
    }
    Ok(if text.contains(SHELL_SPECIAL) {
        format!("\"{text}\"")
    } else {
        text
    })
}

/// Characters that make bash split or expand an unquoted word.
const SHELL_SPECIAL: &[char] = &[
    ' ', '\t', '\'', ';', '&', '|', '<', '>', '(', ')', '*', '?', '[', ']', '{', '}', '~', '#', '!',
];

/// Byte ranges of the whitespace-separated words of a command line, with double-quoted stretches
/// kept inside one word.
fn word_spans(s: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if let Some(st) = start.take() {
                spans.push((st, i));
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        spans.push((st, s.len()));
    }
    spans
}

/// True for a `NAME=value` word, which bash takes as an environment assignment ahead of a command.
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Where the program word starts: past the leading assignments. `None` when there is no program.
fn program_start(command: &str) -> Option<usize> {
    word_spans(command)
        .into_iter()
        .find(|&(a, b)| !is_assignment(&command[a..b]))
        .map(|(a, _)| a)
}

/// The program word and the word after it, past the leading assignments.
fn program_and_next(command: &str) -> Option<(&str, Option<&str>)> {
    let mut words = word_spans(command)
        .into_iter()
        .map(|(a, b)| &command[a..b])
        .skip_while(|w| is_assignment(w));
    let program = words.next()?;
    Some((program, words.next()))
}

fn is_shim_program(word: &str) -> bool {
    let word = word.trim_matches('"');
    let base = word.rsplit(['/', '\\']).next().unwrap_or(word);
    base == "c4-shim" || base == "c4-shim.exe"
}

/// True for a hook command that runs a `c4-shim`, whichever home it belongs to.
#[must_use]
pub fn is_shim_hook_command(command: &str) -> bool {
    program_and_next(command)
        .is_some_and(|(program, next)| is_shim_program(program) && next == Some("hook"))
}

/// The program word of a status line command that already runs through a `c4-shim`, whichever home
/// it belongs to.
#[must_use]
pub fn wrapping_shim(command: &str) -> Option<&str> {
    let (program, next) = program_and_next(command)?;
    (is_shim_program(program) && next == Some("statusline")).then_some(program)
}

/// What `c4 install` did, so `c4 uninstall` can take it back. Lives at
/// `<c4 home>/install-manifest.json`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// The manifest's shape version.
    pub schema: u32,
    /// The C4 that wrote it last.
    pub c4_version: String,
    /// When the first install happened.
    pub installed_at: Timestamp,
    /// When the manifest was last written.
    pub updated_at: Timestamp,
    /// The settings file the install went into.
    pub settings_path: String,
    /// The file before C4 first wrote it.
    pub original: Original,
    /// The SHA-256 of the file as C4 last left it.
    pub after_sha256: String,
    /// True while nothing but C4 has written the file since the original was taken, so uninstall
    /// may put the original back byte for byte.
    pub restorable: bool,
    /// The hook entries that are C4's, by event and command text.
    pub hooks: Vec<HookEntry>,
    /// True when the `hooks` key did not exist before C4 added it.
    pub hooks_created: bool,
    /// The status line command before and after the wrapper, when there was one to wrap.
    pub statusline: Option<StatuslineChange>,
    /// The shim copy.
    pub shim: ShimCopy,
}

/// The settings file as it was before C4 first wrote it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Original {
    /// A copy of the file as it was.
    Backup {
        /// Where the copy is.
        path: String,
        /// The SHA-256 of the copy.
        sha256: String,
    },
    /// There was no file; restoring means removing the one C4 created.
    Missing,
    /// The file was there and needed no change on the first install, so there is nothing to put
    /// back and uninstall removes C4's entries one by one.
    Unchanged {
        /// The SHA-256 of the file at that time.
        sha256: String,
    },
}

/// One hook handler C4 owns.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct HookEntry {
    /// The event, such as `Stop`.
    pub event: String,
    /// The command text, such as `~/.c4/bin/c4-shim hook`.
    pub command: String,
}

/// The status line command before and after the wrapper.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct StatuslineChange {
    /// The command as it was.
    pub before: String,
    /// The command with the shim ahead of it.
    pub after: String,
}

/// The shim copy in the C4 home.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ShimCopy {
    /// Where the copy is.
    pub path: String,
    /// Where it was copied from.
    pub source: String,
    /// The SHA-256 of the copy.
    pub sha256: String,
}

/// Read the manifest under `c4_home`, if there is one.
pub fn read_manifest(c4_home: &C4Home) -> anyhow::Result<Option<Manifest>> {
    let path = c4_home.manifest_path();
    match std::fs::read(&path) {
        Ok(bytes) => {
            let manifest: Manifest = serde_json::from_slice(&bytes).with_context(|| {
                format!(
                    "{} is not a manifest this C4 can read; take C4's entries out of settings.json by hand and remove the manifest to install afresh",
                    path.display()
                )
            })?;
            if manifest.schema > MANIFEST_SCHEMA {
                bail!(
                    "{} has schema {}, written by C4 {}; this C4 ({}) reads schema {}",
                    path.display(),
                    manifest.schema,
                    manifest.c4_version,
                    c4_core::VERSION,
                    MANIFEST_SCHEMA
                );
            }
            Ok(Some(manifest))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

fn write_manifest(c4_home: &C4Home, manifest: &Manifest) -> anyhow::Result<()> {
    std::fs::create_dir_all(c4_home.root())
        .with_context(|| format!("could not create {}", c4_home.root().display()))?;
    let path = c4_home.manifest_path();
    let mut text = serde_json::to_string_pretty(manifest)?;
    text.push('\n');
    let tmp = sibling(&path, ".tmp");
    std::fs::write(&tmp, text).with_context(|| format!("could not write {}", tmp.display()))?;
    rename_retrying(&tmp, &path)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// How the file is laid out, so the rewrite looks like the original.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Layout {
    indent: String,
    crlf: bool,
    trailing_newline: bool,
    bom: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            indent: "  ".into(),
            crlf: false,
            trailing_newline: true,
            bom: false,
        }
    }
}

/// `settings.json` as read: its bytes when it exists, its top-level map, and its layout.
struct SettingsFile {
    bytes: Option<Vec<u8>>,
    map: Map<String, Value>,
    layout: Layout,
}

fn read_settings(path: &Path) -> anyhow::Result<SettingsFile> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let (map, layout) = parse_settings(&bytes, path)?;
            Ok(SettingsFile {
                bytes: Some(bytes),
                map,
                layout,
            })
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SettingsFile {
            bytes: None,
            map: Map::new(),
            layout: Layout::default(),
        }),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

fn parse_settings(bytes: &[u8], path: &Path) -> anyhow::Result<(Map<String, Value>, Layout)> {
    let text = std::str::from_utf8(bytes)
        .with_context(|| format!("{} is not UTF-8; nothing was changed", path.display()))?;
    let (bom, body) = text
        .strip_prefix('\u{feff}')
        .map_or((false, text), |t| (true, t));
    let value: Value = serde_json::from_str(body)
        .with_context(|| format!("{} is not valid JSON; nothing was changed", path.display()))?;
    let Value::Object(map) = value else {
        bail!(
            "{} does not hold a JSON object; nothing was changed",
            path.display()
        );
    };
    let layout = Layout {
        indent: detect_indent(body),
        crlf: body.contains("\r\n"),
        trailing_newline: body.ends_with('\n'),
        bom,
    };
    Ok((map, layout))
}

/// The whitespace of the first indented line, as the unit of indentation; two spaces when nothing
/// is indented.
fn detect_indent(text: &str) -> String {
    text.lines()
        .skip(1)
        .map(|line| {
            let ws: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            (ws, line.len())
        })
        .find(|(ws, len)| !ws.is_empty() && ws.len() < *len)
        .map_or_else(|| "  ".to_string(), |(ws, _)| ws)
}

fn render(map: &Map<String, Value>, layout: &Layout) -> Vec<u8> {
    let mut buf = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(layout.indent.as_bytes());
    let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
    map.serialize(&mut ser).expect("a JSON map serializes");
    let mut text = String::from_utf8(buf).expect("serde_json writes UTF-8");
    if layout.trailing_newline {
        text.push('\n');
    }
    if layout.crlf {
        text = text.replace('\n', "\r\n");
    }
    if layout.bom {
        text.insert(0, '\u{feff}');
    }
    text.into_bytes()
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Rename with a few retries, for Windows, where a file another process holds open cannot be
/// replaced for a moment (a shim run takes milliseconds).
fn rename_retrying(from: &Path, to: &Path) -> anyhow::Result<()> {
    let mut last = None;
    for attempt in 1..=6u64 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(50 * attempt));
            }
        }
    }
    let _ = std::fs::remove_file(from);
    Err(last.expect("six attempts were made")).with_context(|| {
        format!(
            "could not move {} into place as {}",
            from.display(),
            to.display()
        )
    })
}

/// Write `bytes` to a sibling temporary file and rename it over `path`, keeping the permissions of
/// the file that was there.
fn write_settings(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let tmp = sibling(path, ".c4-tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("could not write {}", tmp.display()))?;
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(path) {
        std::fs::set_permissions(&tmp, meta.permissions())
            .with_context(|| format!("could not set the permissions of {}", tmp.display()))?;
    }
    rename_retrying(&tmp, path)
}

/// Write the shim copy, executable, through a sibling temporary file and a rename.
fn write_shim(dest: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    let tmp = sibling(dest, ".c4-tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("could not write {}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))
            .with_context(|| format!("could not make {} executable", tmp.display()))?;
    }
    rename_retrying(&tmp, dest)
}

/// Remove a file with a few retries, for a shim copy that is running at that moment on Windows.
/// `Ok(false)` when the file was not there.
fn remove_retrying(path: &Path) -> anyhow::Result<bool> {
    let mut last = None;
    for attempt in 1..=6u64 {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(50 * attempt));
            }
        }
    }
    Err(last.expect("six attempts were made"))
        .with_context(|| format!("could not remove {}", path.display()))
}

/// The name of the next backup beside `settings`: `settings.json.bak-c4-<UTC stamp>`, with a
/// counter when that name is taken.
fn backup_path(settings: &Path, now: Timestamp) -> PathBuf {
    let name = settings.file_name().map_or_else(
        || "settings.json".to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let dir = settings
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let base = format!("{name}.bak-c4-{}", now.to_file_stamp());
    let mut candidate = dir.join(&base);
    let mut n = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    candidate
}

fn back_up(settings: &Path, now: Timestamp) -> anyhow::Result<PathBuf> {
    let to = backup_path(settings, now);
    std::fs::copy(settings, &to).with_context(|| {
        format!(
            "could not back up {} to {}",
            settings.display(),
            to.display()
        )
    })?;
    Ok(to)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

fn default_shim_source() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("could not find this executable's own path")?;
    let dir = exe
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    Ok(dir.join(shim_file_name()))
}

/// What became of the status line on install.
#[derive(Debug, Default, PartialEq, Eq)]
enum StatuslineOutcome {
    /// No `statusLine` in the file; nothing to wrap.
    #[default]
    Absent,
    /// A `statusLine` that is not a command, or has no command string.
    NotACommand,
    /// A command with no program word.
    Empty,
    /// Already wrapped by this home's shim.
    AlreadyWrapped,
    /// Wrapped by a `c4-shim` of another home, left alone.
    WrappedElsewhere(String),
    /// Wrapped now.
    Wrapped { before: String, after: String },
}

/// What install changed in the map.
#[derive(Debug, Default)]
struct InstallPlan {
    hooks_created: bool,
    added: Vec<&'static str>,
    present: Vec<&'static str>,
    foreign: Vec<String>,
    statusline: StatuslineOutcome,
}

impl InstallPlan {
    fn changes(&self) -> bool {
        !self.added.is_empty() || matches!(self.statusline, StatuslineOutcome::Wrapped { .. })
    }
}

/// The command strings of a hook group's handlers.
fn handlers(group: &Value) -> impl Iterator<Item = &str> {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|h| h.get("command").and_then(Value::as_str))
}

fn hook_group(command: &str, timeout: u64, run_async: bool) -> Value {
    let mut handler = json!({"type": "command", "command": command, "timeout": timeout});
    if run_async {
        handler["async"] = Value::Bool(true);
    }
    json!({"hooks": [handler]})
}

fn wrap_statusline(root: &mut Map<String, Value>, shim: &ShimCommand) -> StatuslineOutcome {
    let present = root.contains_key("statusLine");
    let Some(Value::Object(statusline)) = root.get_mut("statusLine") else {
        return if present {
            StatuslineOutcome::NotACommand
        } else {
            StatuslineOutcome::Absent
        };
    };
    if statusline
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t != "command")
    {
        return StatuslineOutcome::NotACommand;
    }
    let Some(Value::String(command)) = statusline.get_mut("command") else {
        return StatuslineOutcome::NotACommand;
    };
    if let Some(program) = wrapping_shim(command) {
        return if command.contains(&shim.statusline_segment()) {
            StatuslineOutcome::AlreadyWrapped
        } else {
            StatuslineOutcome::WrappedElsewhere(program.to_string())
        };
    }
    match shim.wrap(command) {
        Some(wrapped) => {
            let before = std::mem::replace(command, wrapped.clone());
            StatuslineOutcome::Wrapped {
                before,
                after: wrapped,
            }
        }
        None => StatuslineOutcome::Empty,
    }
}

fn apply_install(root: &mut Map<String, Value>, shim: &ShimCommand) -> anyhow::Result<InstallPlan> {
    let statusline = wrap_statusline(root, shim);
    let hooks_created = !root.contains_key("hooks");
    let mut plan = InstallPlan {
        hooks_created,
        statusline,
        ..InstallPlan::default()
    };
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(hooks) = hooks else {
        bail!("\"hooks\" in settings.json is not an object; nothing was changed");
    };
    let command = shim.hook();
    for (event, timeout, run_async) in HOOK_EVENTS {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(groups) = groups else {
            bail!("\"hooks\".\"{event}\" in settings.json is not an array; nothing was changed");
        };
        let mut found = false;
        for handler in groups.iter().flat_map(handlers) {
            if handler == command {
                found = true;
            } else if is_shim_hook_command(handler) {
                plan.foreign.push(format!("{event}: {handler}"));
            }
        }
        if found {
            plan.present.push(event);
        } else {
            groups.push(hook_group(&command, timeout, run_async));
            plan.added.push(event);
        }
    }
    Ok(plan)
}

/// The hook entries C4 owns after this install: the previous manifest's plus every event that now
/// carries this home's command.
fn merge_hooks(previous: Vec<HookEntry>, shim: &ShimCommand) -> Vec<HookEntry> {
    let mut hooks = previous;
    let command = shim.hook();
    for (event, _, _) in HOOK_EVENTS {
        if !hooks
            .iter()
            .any(|h| h.event == event && h.command == command)
        {
            hooks.push(HookEntry {
                event: event.to_string(),
                command: command.clone(),
            });
        }
    }
    hooks
}

struct InstallReport {
    dry_run: bool,
    claude_home: String,
    claude_source: &'static str,
    c4_home: String,
    c4_source: &'static str,
    shim_source: String,
    shim_dest: String,
    shim_current: bool,
    plan: InstallPlan,
    backup: Option<String>,
    manifest_path: String,
    restorable: bool,
}

fn install(args: &InstallArgs) -> anyhow::Result<InstallReport> {
    let now = Timestamp::now();
    let claude_home = args.home.resolve();
    if !claude_home.exists() {
        bail!(
            "the Claude home {} does not exist; start Claude Code once, or point --home at it",
            claude_home.root().display()
        );
    }
    let settings_path = claude_home.settings_path();
    let c4_home = args.c4_home.resolve();
    let shim_source = if let Some(p) = &args.shim {
        p.clone()
    } else {
        default_shim_source()?
    };
    let shim_bytes = std::fs::read(&shim_source).with_context(|| {
        format!(
            "no shim at {}; build it with `cargo build -p c4-shim --release` or pass --shim",
            shim_source.display()
        )
    })?;
    let shim = ShimCommand::for_home(&c4_home, &user_home())?;
    let previous = read_manifest(&c4_home)?;
    if let Some(prev) = &previous
        && !same_file(Path::new(&prev.settings_path), &settings_path)
    {
        bail!(
            "the manifest under {} records an install into {}; run c4 uninstall there first, or use another --c4-home",
            c4_home.root().display(),
            prev.settings_path
        );
    }
    let SettingsFile {
        bytes: before_bytes,
        mut map,
        layout,
    } = read_settings(&settings_path)?;
    let before_sha = before_bytes.as_deref().map(sha256_hex);
    let plan = apply_install(&mut map, &shim)?;
    let shim_dest = c4_home.bin_dir().join(shim_file_name());
    let shim_current = std::fs::read(&shim_dest).is_ok_and(|b| b == shim_bytes);
    let restorable_now = match &previous {
        Some(prev) => prev.restorable && before_sha.as_deref() == Some(prev.after_sha256.as_str()),
        None => before_bytes.is_none() || plan.changes(),
    };
    let mut report = InstallReport {
        dry_run: args.dry_run,
        claude_home: claude_home.root().display().to_string(),
        claude_source: args.home.source(),
        c4_home: c4_home.root().display().to_string(),
        c4_source: args.c4_home.source(),
        shim_source: shim_source.display().to_string(),
        shim_dest: shim_dest.display().to_string(),
        shim_current,
        plan,
        backup: None,
        manifest_path: c4_home.manifest_path().display().to_string(),
        restorable: restorable_now,
    };
    if args.dry_run {
        return Ok(report);
    }
    if !shim_current {
        write_shim(&shim_dest, &shim_bytes)?;
    }
    let after_bytes = if report.plan.changes() {
        if before_bytes.is_some() {
            let backup = back_up(&settings_path, now)?;
            report.backup = Some(backup.display().to_string());
        }
        let rendered = render(&map, &layout);
        write_settings(&settings_path, &rendered)?;
        rendered
    } else {
        before_bytes.unwrap_or_default()
    };
    let after_sha256 = sha256_hex(&after_bytes);
    let shim_copy = ShimCopy {
        path: report.shim_dest.clone(),
        source: report.shim_source.clone(),
        sha256: sha256_hex(&shim_bytes),
    };
    let statusline_change = match &report.plan.statusline {
        StatuslineOutcome::Wrapped { before, after } => Some(StatuslineChange {
            before: before.clone(),
            after: after.clone(),
        }),
        _ => None,
    };
    let manifest = if let Some(prev) = previous {
        Manifest {
            schema: MANIFEST_SCHEMA,
            c4_version: c4_core::VERSION.to_string(),
            installed_at: prev.installed_at,
            updated_at: now,
            settings_path: prev.settings_path,
            original: prev.original,
            after_sha256,
            restorable: restorable_now,
            hooks: merge_hooks(prev.hooks, &shim),
            hooks_created: prev.hooks_created || report.plan.hooks_created,
            statusline: statusline_change.or(prev.statusline),
            shim: shim_copy,
        }
    } else {
        let original = match (&before_sha, &report.backup) {
            (None, _) => Original::Missing,
            (Some(sha256), Some(path)) => Original::Backup {
                path: path.clone(),
                sha256: sha256.clone(),
            },
            (Some(sha256), None) => Original::Unchanged {
                sha256: sha256.clone(),
            },
        };
        Manifest {
            schema: MANIFEST_SCHEMA,
            c4_version: c4_core::VERSION.to_string(),
            installed_at: now,
            updated_at: now,
            settings_path: settings_path.display().to_string(),
            original,
            after_sha256,
            restorable: restorable_now,
            hooks: merge_hooks(Vec::new(), &shim),
            hooks_created: report.plan.hooks_created,
            statusline: statusline_change,
            shim: shim_copy,
        }
    };
    write_manifest(&c4_home, &manifest)?;
    Ok(report)
}

fn tense<'a>(dry_run: bool, would: &'a str, did: &'a str) -> &'a str {
    if dry_run { would } else { did }
}

fn write_install(out: &mut dyn Write, r: &InstallReport) -> std::io::Result<()> {
    let dry = r.dry_run;
    writeln!(
        out,
        "c4 install{}",
        if dry {
            " (dry run, nothing written)"
        } else {
            ""
        }
    )?;
    writeln!(
        out,
        "  claude home: {} (from {})",
        redact(&r.claude_home),
        r.claude_source
    )?;
    writeln!(
        out,
        "  c4 home: {} (from {})",
        redact(&r.c4_home),
        r.c4_source
    )?;
    if r.shim_current {
        writeln!(out, "  shim: {} is current", redact(&r.shim_dest))?;
    } else {
        writeln!(
            out,
            "  shim: {} {} to {}",
            tense(dry, "would copy", "copied"),
            redact(&r.shim_source),
            redact(&r.shim_dest)
        )?;
    }
    match &r.plan.statusline {
        StatuslineOutcome::Wrapped { after, .. } => writeln!(
            out,
            "  statusLine: {}: {}",
            tense(dry, "would wrap", "wrapped"),
            redact(after)
        )?,
        StatuslineOutcome::AlreadyWrapped => writeln!(out, "  statusLine: already wrapped")?,
        StatuslineOutcome::WrappedElsewhere(program) => writeln!(
            out,
            "  statusLine: wrapped by another c4-shim ({}), left alone",
            redact(program)
        )?,
        StatuslineOutcome::Absent => {
            writeln!(out, "  statusLine: none in settings.json, nothing to wrap")?;
        }
        StatuslineOutcome::NotACommand => writeln!(
            out,
            "  statusLine: not a command with a command string, left alone"
        )?,
        StatuslineOutcome::Empty => {
            writeln!(
                out,
                "  statusLine: the command names no program, left alone"
            )?;
        }
    }
    let added = r.plan.added.len();
    let present = r.plan.present.len();
    if added == 0 {
        writeln!(out, "  hooks: all {present} groups present")?;
    } else if present == 0 {
        writeln!(
            out,
            "  hooks: {} {added} groups: {}",
            tense(dry, "would add", "added"),
            r.plan.added.join(", ")
        )?;
    } else {
        writeln!(
            out,
            "  hooks: {} {added} groups ({}), {present} present",
            tense(dry, "would add", "added"),
            r.plan.added.join(", ")
        )?;
    }
    if r.plan.hooks_created {
        writeln!(
            out,
            "  hooks: the file had no \"hooks\" key; {}",
            tense(dry, "it would be created", "it was created")
        )?;
    }
    if r.plan.changes() {
        match (&r.backup, dry) {
            (_, true) => writeln!(out, "  settings.json: would be written after a backup")?,
            (Some(b), false) => writeln!(out, "  settings.json: written, backup {}", redact(b))?,
            (None, false) => writeln!(out, "  settings.json: created")?,
        }
    } else {
        writeln!(out, "  settings.json: nothing to change")?;
    }
    writeln!(
        out,
        "  manifest: {} {} (restorable: {})",
        redact(&r.manifest_path),
        tense(dry, "would be written", "written"),
        if r.restorable { "yes" } else { "no" }
    )?;
    for foreign in &r.plan.foreign {
        writeln!(
            out,
            "  note: a hook of another c4-shim is also installed, left alone: {}",
            redact(foreign)
        )?;
    }
    Ok(())
}

/// Run `c4 install`.
pub fn run_install(args: &InstallArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let report = install(args)?;
    write_install(out, &report)?;
    out.flush()?;
    Ok(())
}

/// What became of the status line on uninstall.
#[derive(Debug, Default, PartialEq, Eq)]
enum StripStatusline {
    /// No command to look at.
    #[default]
    Absent,
    /// Not wrapped by this home's shim.
    NotWrapped,
    /// Unwrapped now.
    Unwrapped { before: String, after: String },
}

/// What uninstall removed from the map.
#[derive(Debug, Default, PartialEq, Eq)]
struct StripPlan {
    handlers_removed: usize,
    events: Vec<String>,
    events_dropped: Vec<String>,
    hooks_dropped: bool,
    statusline: StripStatusline,
}

impl StripPlan {
    fn changes(&self) -> bool {
        self.handlers_removed > 0 || matches!(self.statusline, StripStatusline::Unwrapped { .. })
    }
}

fn unwrap_statusline(
    root: &mut Map<String, Value>,
    shim: Option<&ShimCommand>,
    manifest: Option<&Manifest>,
) -> StripStatusline {
    let Some(Value::Object(statusline)) = root.get_mut("statusLine") else {
        return StripStatusline::Absent;
    };
    let Some(Value::String(command)) = statusline.get_mut("command") else {
        return StripStatusline::Absent;
    };
    let recorded = manifest
        .and_then(|m| m.statusline.as_ref())
        .filter(|s| s.after == *command)
        .map(|s| s.before.clone());
    let unwrapped = recorded.or_else(|| shim.and_then(|s| s.unwrap(command)));
    match unwrapped {
        Some(after) => {
            let before = std::mem::replace(command, after.clone());
            StripStatusline::Unwrapped { before, after }
        }
        None => StripStatusline::NotWrapped,
    }
}

fn count_handlers(groups: &[Value]) -> usize {
    groups.iter().map(|g| handlers(g).count()).sum()
}

fn apply_uninstall(
    root: &mut Map<String, Value>,
    shim: Option<&ShimCommand>,
    manifest: Option<&Manifest>,
) -> StripPlan {
    let statusline = unwrap_statusline(root, shim, manifest);
    let mut plan = StripPlan {
        statusline,
        ..StripPlan::default()
    };
    let mut commands: Vec<String> = manifest
        .map(|m| m.hooks.iter().map(|h| h.command.clone()).collect())
        .unwrap_or_default();
    if let Some(shim) = shim {
        commands.push(shim.hook());
    }
    commands.sort();
    commands.dedup();
    if let Some(Value::Object(hooks)) = root.get_mut("hooks") {
        let mut dropped = Vec::new();
        for (event, groups) in &mut *hooks {
            let Value::Array(groups) = groups else {
                continue;
            };
            let before = count_handlers(groups);
            groups.retain_mut(|group| {
                let Some(Value::Array(handlers)) = group.get_mut("hooks") else {
                    return true;
                };
                handlers.retain(|h| {
                    !h.get("command")
                        .and_then(Value::as_str)
                        .is_some_and(|c| commands.iter().any(|x| x == c))
                });
                !handlers.is_empty()
            });
            let removed = before - count_handlers(groups);
            if removed > 0 {
                plan.handlers_removed += removed;
                plan.events.push(event.clone());
                if groups.is_empty() {
                    dropped.push(event.clone());
                }
            }
        }
        for event in &dropped {
            hooks.shift_remove(event);
        }
        plan.events_dropped = dropped;
        if hooks.is_empty() && manifest.is_some_and(|m| m.hooks_created) {
            root.shift_remove("hooks");
            plan.hooks_dropped = true;
        }
    }
    plan
}

/// How uninstall puts the settings file right.
#[derive(Debug, PartialEq, Eq)]
enum Removal {
    /// Write the original backup's bytes over the file.
    Restore(String),
    /// Remove the file C4 created.
    RemoveFile,
    /// Take C4's entries out one by one and keep everything else.
    Strip(StripPlan),
    /// Nothing of C4's is in the file, or there is no file.
    Nothing,
}

struct UninstallReport {
    dry_run: bool,
    claude_home: String,
    claude_source: &'static str,
    c4_home: String,
    c4_source: &'static str,
    had_manifest: bool,
    removal: Removal,
    backup: Option<String>,
    shim_dest: String,
    shim_removed: bool,
    notes: Vec<String>,
}

fn uninstall(args: &UninstallArgs) -> anyhow::Result<UninstallReport> {
    let now = Timestamp::now();
    let claude_home = args.home.resolve();
    let settings_path = claude_home.settings_path();
    let c4_home = args.c4_home.resolve();
    let manifest = read_manifest(&c4_home)?;
    // A home path bash could not read is no reason to refuse a removal.
    let shim = ShimCommand::for_home(&c4_home, &user_home()).ok();
    let mut notes = Vec::new();
    let bytes = match std::fs::read(&settings_path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(e).with_context(|| format!("could not read {}", settings_path.display()));
        }
    };
    let current_sha = bytes.as_deref().map(sha256_hex);
    let untouched_since_install = manifest
        .as_ref()
        .is_some_and(|m| m.restorable && current_sha.as_deref() == Some(m.after_sha256.as_str()));
    let mut removal = Removal::Nothing;
    if untouched_since_install {
        match &manifest.as_ref().expect("checked above").original {
            Original::Backup { path, sha256 } => match std::fs::read(path) {
                Ok(b) if sha256_hex(&b) == *sha256 => removal = Removal::Restore(path.clone()),
                Ok(_) => notes.push(format!(
                    "the backup {path} no longer matches the manifest; C4's entries are removed one by one instead"
                )),
                Err(_) => notes.push(format!(
                    "the backup {path} is gone; C4's entries are removed one by one instead"
                )),
            },
            Original::Missing => removal = Removal::RemoveFile,
            Original::Unchanged { .. } => {}
        }
    }
    let mut rendered = None;
    if removal == Removal::Nothing {
        if let Some(bytes) = &bytes {
            let (mut map, layout) = parse_settings(bytes, &settings_path)?;
            let plan = apply_uninstall(&mut map, shim.as_ref(), manifest.as_ref());
            if plan.changes() {
                rendered = Some(render(&map, &layout));
                removal = Removal::Strip(plan);
            } else {
                notes.push("settings.json holds nothing of C4's".to_string());
            }
        } else {
            notes.push("there is no settings.json under the Claude home".to_string());
        }
    }
    let shim_dest = c4_home.bin_dir().join(shim_file_name());
    let mut report = UninstallReport {
        dry_run: args.dry_run,
        claude_home: claude_home.root().display().to_string(),
        claude_source: args.home.source(),
        c4_home: c4_home.root().display().to_string(),
        c4_source: args.c4_home.source(),
        had_manifest: manifest.is_some(),
        removal,
        backup: None,
        shim_dest: shim_dest.display().to_string(),
        shim_removed: shim_dest.is_file(),
        notes,
    };
    if args.dry_run {
        return Ok(report);
    }
    match &report.removal {
        Removal::Restore(path) => {
            report.backup = Some(back_up(&settings_path, now)?.display().to_string());
            let original = std::fs::read(path)?;
            write_settings(&settings_path, &original)?;
        }
        Removal::RemoveFile => {
            report.backup = Some(back_up(&settings_path, now)?.display().to_string());
            remove_retrying(&settings_path)?;
        }
        Removal::Strip(_) => {
            report.backup = Some(back_up(&settings_path, now)?.display().to_string());
            write_settings(
                &settings_path,
                rendered.as_deref().expect("a strip renders the file"),
            )?;
        }
        Removal::Nothing => {}
    }
    match remove_retrying(&shim_dest) {
        Ok(removed) => report.shim_removed = removed,
        Err(e) => {
            report.shim_removed = false;
            report
                .notes
                .push(format!("{e:#}; remove it by hand once no shim is running"));
        }
    }
    if manifest.is_some() {
        remove_retrying(&c4_home.manifest_path())?;
    }
    Ok(report)
}

fn write_uninstall(out: &mut dyn Write, r: &UninstallReport) -> std::io::Result<()> {
    let dry = r.dry_run;
    writeln!(
        out,
        "c4 uninstall{}",
        if dry {
            " (dry run, nothing written)"
        } else {
            ""
        }
    )?;
    writeln!(
        out,
        "  claude home: {} (from {})",
        redact(&r.claude_home),
        r.claude_source
    )?;
    writeln!(
        out,
        "  c4 home: {} (from {})",
        redact(&r.c4_home),
        r.c4_source
    )?;
    match &r.removal {
        Removal::Restore(path) => writeln!(
            out,
            "  settings.json: {} from {} (nothing else had written the file since c4 install)",
            tense(dry, "would be restored", "restored"),
            redact(path)
        )?,
        Removal::RemoveFile => writeln!(
            out,
            "  settings.json: {} (it did not exist before c4 install)",
            tense(dry, "would be removed", "removed")
        )?,
        Removal::Strip(plan) => {
            writeln!(
                out,
                "  settings.json: {} {} hook handlers from {} events{}; other changes kept",
                tense(dry, "would remove", "removed"),
                plan.handlers_removed,
                plan.events.len(),
                match &plan.statusline {
                    StripStatusline::Unwrapped { .. } => " and the statusLine wrapper",
                    _ => "",
                }
            )?;
            if !plan.events_dropped.is_empty() {
                writeln!(
                    out,
                    "  hooks: {} emptied and {}: {}",
                    plan.events_dropped.len(),
                    tense(dry, "would be dropped", "dropped"),
                    plan.events_dropped.join(", ")
                )?;
            }
            if plan.hooks_dropped {
                writeln!(
                    out,
                    "  hooks: the key C4 created {}",
                    tense(dry, "would be dropped", "was dropped")
                )?;
            }
        }
        Removal::Nothing => writeln!(out, "  settings.json: nothing to remove")?,
    }
    if let Some(b) = &r.backup {
        writeln!(out, "  backup: {}", redact(b))?;
    } else if dry && r.removal != Removal::Nothing {
        writeln!(out, "  backup: would be taken first")?;
    }
    if r.shim_removed {
        writeln!(
            out,
            "  shim: {} {}",
            tense(dry, "would remove", "removed"),
            redact(&r.shim_dest)
        )?;
    } else {
        writeln!(out, "  shim: no copy at {}", redact(&r.shim_dest))?;
    }
    let manifest = match (r.had_manifest, dry) {
        (false, _) => "none found",
        (true, true) => "would be removed",
        (true, false) => "removed",
    };
    writeln!(out, "  manifest: {manifest}")?;
    for note in &r.notes {
        writeln!(out, "  note: {}", redact(note))?;
    }
    Ok(())
}

/// Run `c4 uninstall`.
pub fn run_uninstall(args: &UninstallArgs, out: &mut dyn Write) -> anyhow::Result<()> {
    let report = uninstall(args)?;
    write_uninstall(out, &report)?;
    out.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_home() -> PathBuf {
        PathBuf::from("/home/example")
    }

    fn default_shim() -> ShimCommand {
        ShimCommand::for_home(&C4Home::at(example_home().join(".c4")), &example_home()).unwrap()
    }

    fn exe() -> String {
        shim_file_name()
    }

    #[test]
    fn the_default_home_is_named_with_a_tilde() {
        let shim = default_shim();
        assert_eq!(shim.hook(), format!("~/.c4/bin/{} hook", exe()));
        assert_eq!(
            shim.statusline_segment(),
            format!("~/.c4/bin/{} statusline -- ", exe())
        );
        // A trailing separator or, on Windows, another case is still the default home.
        let spelled = ShimCommand::for_home(
            &C4Home::at(format!("{}/.c4/", example_home().display())),
            &example_home(),
        )
        .unwrap();
        assert_eq!(spelled, shim);
    }

    #[test]
    fn another_home_carries_the_assignment_and_quotes_when_needed() {
        let (root, home_text, program) = if cfg!(windows) {
            (
                PathBuf::from(r"C:\tmp\c4 home"),
                "\"C:/tmp/c4 home\"",
                "\"C:/tmp/c4 home/bin/c4-shim.exe\"",
            )
        } else {
            (
                PathBuf::from("/tmp/c4 home"),
                "\"/tmp/c4 home\"",
                "\"/tmp/c4 home/bin/c4-shim\"",
            )
        };
        let shim = ShimCommand::for_home(&C4Home::at(root), &example_home()).unwrap();
        assert_eq!(shim.hook(), format!("C4_HOME={home_text} {program} hook"));
        let plain = if cfg!(windows) {
            PathBuf::from(r"C:\c4")
        } else {
            PathBuf::from("/opt/c4")
        };
        let shim = ShimCommand::for_home(&C4Home::at(&plain), &example_home()).unwrap();
        let plain_text = plain.to_string_lossy().replace('\\', "/");
        assert_eq!(
            shim.hook(),
            format!("C4_HOME={plain_text} {plain_text}/bin/{} hook", exe())
        );
    }

    #[test]
    fn a_path_bash_would_interpret_is_refused() {
        let bad = if cfg!(windows) {
            PathBuf::from(r"C:\tmp\c4$home")
        } else {
            PathBuf::from("/tmp/c4$home")
        };
        let err = ShimCommand::for_home(&C4Home::at(bad), &example_home()).unwrap_err();
        assert!(err.to_string().contains("dollar sign"), "{err}");
    }

    #[test]
    fn wrap_keeps_leading_assignments_and_unwrap_is_its_inverse() {
        let shim = default_shim();
        let segment = shim.statusline_segment();
        let original = "PYTHONIOENCODING=utf-8 python ~/.claude/statusline.py";
        let wrapped = shim.wrap(original).unwrap();
        assert_eq!(
            wrapped,
            format!("PYTHONIOENCODING=utf-8 {segment}python ~/.claude/statusline.py")
        );
        assert_eq!(shim.unwrap(&wrapped).as_deref(), Some(original));
        assert_eq!(shim.wrap("python x").unwrap(), format!("{segment}python x"));
        assert_eq!(
            shim.wrap("A=1 B=\"two words\" prog arg").unwrap(),
            format!("A=1 B=\"two words\" {segment}prog arg")
        );
        assert_eq!(shim.wrap(""), None);
        assert_eq!(shim.wrap("A=1"), None);
        assert_eq!(shim.unwrap("python x"), None);
    }

    #[test]
    fn shim_commands_are_recognized_whichever_home_they_name() {
        assert!(is_shim_hook_command("~/.c4/bin/c4-shim hook"));
        assert!(is_shim_hook_command("~/.c4/bin/c4-shim.exe hook"));
        assert!(is_shim_hook_command("C4_HOME=/x /x/bin/c4-shim hook"));
        assert!(is_shim_hook_command(
            "C4_HOME=\"/x y\" \"/x y/bin/c4-shim.exe\" hook"
        ));
        assert!(!is_shim_hook_command("python ~/.claude/hooks/turn_end.py"));
        assert!(!is_shim_hook_command("~/.c4/bin/c4-shim statusline -- x"));
        assert!(!is_shim_hook_command(""));
        assert_eq!(
            wrapping_shim("PYTHONIOENCODING=utf-8 ~/.c4/bin/c4-shim statusline -- python x"),
            Some("~/.c4/bin/c4-shim")
        );
        assert_eq!(wrapping_shim("python x"), None);
        assert_eq!(wrapping_shim("~/.c4/bin/c4-shim hook"), None);
    }

    fn sample() -> (Map<String, Value>, Layout) {
        let text = r#"{
  "permissions": {"allow": ["Bash(git status:*)"]},
  "statusLine": {"type": "command", "command": "PYTHONIOENCODING=utf-8 python ~/.claude/statusline.py", "padding": 1},
  "hooks": {
    "Stop": [{"hooks": [{"type": "command", "command": "python ~/.claude/hooks/turn_end.py", "timeout": 60}]}]
  },
  "model": "claude-opus-5-5"
}
"#;
        parse_settings(text.as_bytes(), Path::new("settings.json")).unwrap()
    }

    fn keys(map: &Map<String, Value>) -> Vec<&str> {
        map.keys().map(String::as_str).collect()
    }

    #[test]
    fn install_then_uninstall_round_trips_the_map() {
        let shim = default_shim();
        let (original, _) = sample();
        let mut map = original.clone();
        let plan = apply_install(&mut map, &shim).unwrap();
        assert_eq!(plan.added.len(), 17);
        assert_eq!(plan.present, Vec::<&str>::new());
        assert!(!plan.hooks_created);
        assert_eq!(plan.foreign, Vec::<String>::new());
        assert!(matches!(plan.statusline, StatuslineOutcome::Wrapped { .. }));
        assert_eq!(keys(&map), keys(&original), "top-level key order");
        let hooks = map["hooks"].as_object().unwrap();
        assert_eq!(keys(hooks)[0], "Stop", "the existing event stays first");
        assert_eq!(hooks.len(), 17);
        let stop = hooks["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
        assert_eq!(
            stop[0]["hooks"][0]["command"],
            "python ~/.claude/hooks/turn_end.py"
        );
        assert_eq!(stop[1]["hooks"][0]["command"], shim.hook());
        assert_eq!(stop[1]["hooks"][0]["async"], true);
        assert_eq!(stop[1]["hooks"][0]["timeout"], 5);
        let end = hooks["SessionEnd"].as_array().unwrap();
        assert_eq!(end.len(), 1);
        assert_eq!(end[0]["hooks"][0]["timeout"], 1);
        assert!(end[0]["hooks"][0].get("async").is_none());
        assert_eq!(
            map["statusLine"]["command"],
            format!(
                "PYTHONIOENCODING=utf-8 {}python ~/.claude/statusline.py",
                shim.statusline_segment()
            )
        );

        // A second time: nothing to add.
        let again = apply_install(&mut map.clone(), &shim).unwrap();
        assert_eq!(again.added, Vec::<&str>::new());
        assert_eq!(again.present.len(), 17);
        assert_eq!(again.statusline, StatuslineOutcome::AlreadyWrapped);
        assert!(!again.changes());

        // Out again, without a manifest: this home's command text is enough.
        let plan = apply_uninstall(&mut map, Some(&shim), None);
        assert_eq!(plan.handlers_removed, 17);
        assert_eq!(plan.events.len(), 17);
        assert_eq!(plan.events_dropped.len(), 16, "Stop keeps its own group");
        assert!(!plan.events_dropped.iter().any(|e| e == "Stop"));
        assert!(!plan.hooks_dropped);
        assert!(matches!(plan.statusline, StripStatusline::Unwrapped { .. }));
        assert_eq!(map, original);
        assert_eq!(keys(&map), keys(&original));
    }

    #[test]
    fn a_file_without_hooks_or_status_line_gets_hooks_alone() {
        let shim = default_shim();
        let mut map = Map::new();
        map.insert("model".into(), Value::String("claude-opus-5-5".into()));
        let plan = apply_install(&mut map, &shim).unwrap();
        assert!(plan.hooks_created);
        assert_eq!(plan.statusline, StatuslineOutcome::Absent);
        assert_eq!(keys(&map), ["model", "hooks"]);
        let manifest = Manifest {
            schema: MANIFEST_SCHEMA,
            c4_version: c4_core::VERSION.into(),
            installed_at: Timestamp::from_millis(0),
            updated_at: Timestamp::from_millis(0),
            settings_path: "settings.json".into(),
            original: Original::Missing,
            after_sha256: String::new(),
            restorable: true,
            hooks: merge_hooks(Vec::new(), &shim),
            hooks_created: true,
            statusline: None,
            shim: ShimCopy {
                path: String::new(),
                source: String::new(),
                sha256: String::new(),
            },
        };
        let plan = apply_uninstall(&mut map, Some(&shim), Some(&manifest));
        assert!(plan.hooks_dropped);
        assert_eq!(plan.statusline, StripStatusline::Absent);
        assert_eq!(keys(&map), ["model"]);
    }

    #[test]
    fn foreign_shim_entries_are_reported_and_kept() {
        let shim = default_shim();
        let other = "C4_HOME=/srv/other /srv/other/bin/c4-shim hook";
        let mut map = Map::new();
        map.insert(
            "hooks".into(),
            json!({"Stop": [{"hooks": [{"type": "command", "command": other, "timeout": 5, "async": true}]}]}),
        );
        let plan = apply_install(&mut map, &shim).unwrap();
        assert_eq!(plan.foreign, vec![format!("Stop: {other}")]);
        assert_eq!(plan.added.len(), 17);
        let plan = apply_uninstall(&mut map, Some(&shim), None);
        assert_eq!(plan.handlers_removed, 17);
        let stop = map["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 1);
        assert_eq!(stop[0]["hooks"][0]["command"], other);
    }

    #[test]
    fn a_status_line_wrapped_elsewhere_is_left_alone() {
        let shim = default_shim();
        let mut map = Map::new();
        map.insert(
            "statusLine".into(),
            json!({"type": "command", "command": "C4_HOME=/srv/other /srv/other/bin/c4-shim statusline -- python x"}),
        );
        let plan = apply_install(&mut map, &shim).unwrap();
        assert_eq!(
            plan.statusline,
            StatuslineOutcome::WrappedElsewhere("/srv/other/bin/c4-shim".into())
        );
        assert!(
            map["statusLine"]["command"]
                .as_str()
                .unwrap()
                .starts_with("C4_HOME=/srv/other")
        );
        let plan = apply_uninstall(&mut map, Some(&shim), None);
        assert_eq!(plan.statusline, StripStatusline::NotWrapped);
    }

    #[test]
    fn hooks_that_are_not_an_object_stop_the_install() {
        let shim = default_shim();
        let mut map = Map::new();
        map.insert("hooks".into(), Value::Array(Vec::new()));
        let err = apply_install(&mut map, &shim).unwrap_err();
        assert!(err.to_string().contains("not an object"), "{err}");
        let mut map = Map::new();
        map.insert("hooks".into(), json!({"Stop": {}}));
        let err = apply_install(&mut map, &shim).unwrap_err();
        assert!(err.to_string().contains("not an array"), "{err}");
    }

    #[test]
    fn the_layout_is_detected_and_reproduced() {
        let text =
            "\u{feff}{\r\n    \"a\": 1,\r\n    \"b\": {\r\n        \"c\": [1, 2]\r\n    }\r\n}";
        let (map, layout) = parse_settings(text.as_bytes(), Path::new("x.json")).unwrap();
        assert_eq!(
            layout,
            Layout {
                indent: "    ".into(),
                crlf: true,
                trailing_newline: false,
                bom: true
            }
        );
        let rendered = render(&map, &layout);
        assert_eq!(
            String::from_utf8(rendered).unwrap(),
            "\u{feff}{\r\n    \"a\": 1,\r\n    \"b\": {\r\n        \"c\": [\r\n            1,\r\n            2\r\n        ]\r\n    }\r\n}"
        );
        let text = "{\n  \"a\": 1\n}\n";
        let (map, layout) = parse_settings(text.as_bytes(), Path::new("x.json")).unwrap();
        assert_eq!(layout, Layout::default());
        assert_eq!(render(&map, &layout), text.as_bytes());
        let (_, layout) = parse_settings(b"{}", Path::new("x.json")).unwrap();
        assert_eq!(layout.indent, "  ");
        assert!(!layout.trailing_newline);
        let tabs = "{\n\t\"a\": 1\n}\n";
        let (map, layout) = parse_settings(tabs.as_bytes(), Path::new("x.json")).unwrap();
        assert_eq!(layout.indent, "\t");
        assert_eq!(render(&map, &layout), tabs.as_bytes());
    }

    #[test]
    fn broken_settings_are_refused_before_anything_changes() {
        let err = parse_settings(b"{ not json", Path::new("s.json")).unwrap_err();
        assert!(err.to_string().contains("not valid JSON"), "{err}");
        let err = parse_settings(b"[1, 2]", Path::new("s.json")).unwrap_err();
        assert!(err.to_string().contains("JSON object"), "{err}");
    }

    #[test]
    fn backup_names_carry_the_stamp_and_never_collide() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, "{}").unwrap();
        let at = Timestamp::parse_rfc3339("2026-10-26T05:15:30Z").unwrap();
        let first = backup_path(&settings, at);
        assert_eq!(
            first.file_name().unwrap().to_str().unwrap(),
            "settings.json.bak-c4-20261026T051530Z"
        );
        std::fs::write(&first, "{}").unwrap();
        let second = backup_path(&settings, at);
        assert_eq!(
            second.file_name().unwrap().to_str().unwrap(),
            "settings.json.bak-c4-20261026T051530Z-1"
        );
        let copied = back_up(&settings, at).unwrap();
        assert_eq!(copied, second);
        assert_eq!(std::fs::read(&copied).unwrap(), b"{}");
    }

    #[test]
    fn the_manifest_round_trips_through_json() {
        let manifest = Manifest {
            schema: MANIFEST_SCHEMA,
            c4_version: c4_core::VERSION.into(),
            installed_at: Timestamp::from_millis(1_792_972_800_123),
            updated_at: Timestamp::from_millis(1_792_972_800_456),
            settings_path: "/home/example/.claude/settings.json".into(),
            original: Original::Backup {
                path: "/home/example/.claude/settings.json.bak-c4-20261026T051530Z".into(),
                sha256: "ab".repeat(32),
            },
            after_sha256: "cd".repeat(32),
            restorable: true,
            hooks: merge_hooks(Vec::new(), &default_shim()),
            hooks_created: false,
            statusline: Some(StatuslineChange {
                before: "python x".into(),
                after: "~/.c4/bin/c4-shim statusline -- python x".into(),
            }),
            shim: ShimCopy {
                path: "/home/example/.c4/bin/c4-shim".into(),
                source: "/opt/c4/c4-shim".into(),
                sha256: "ef".repeat(32),
            },
        };
        let text = serde_json::to_string_pretty(&manifest).unwrap();
        assert!(text.contains("\"kind\": \"backup\""), "{text}");
        assert!(text.contains("\"installed_at\": 1792972800123"), "{text}");
        let back: Manifest = serde_json::from_str(&text).unwrap();
        assert_eq!(back, manifest);
        let missing: Original = serde_json::from_str(r#"{"kind": "missing"}"#).unwrap();
        assert_eq!(missing, Original::Missing);
        assert_eq!(merge_hooks(back.hooks.clone(), &default_shim()).len(), 17);
    }

    #[test]
    fn sha256_is_the_usual_one() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
