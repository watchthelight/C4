// <wtl> project: c4 | file: crates/c4-core/src/home.rs | author: watchthelight </wtl>
//! Where C4 keeps its own files: `C4_HOME`, else `.c4` under the user's home. Only path arithmetic
//! lives here; nothing is created or read. The shim, which depends on no crate, repeats the endpoint
//! rule in its own code and a test in its crate keeps the two in step.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Overrides the C4 home. Tests point it at a scratch directory.
pub const HOME_ENV: &str = "C4_HOME";
/// The named pipe prefix on Windows; the hash of the home path follows it, so two homes on one
/// machine never share a pipe. `LOCAL` keeps the name to the login session, the convention Claude
/// Code's own peer-message pipes follow.
pub const PIPE_PREFIX: &str = r"\\.\pipe\LOCAL\c4-ingest-";
/// The socket file under the home on Unix.
pub const SOCKET_NAME: &str = "ingest.sock";

/// The root C4 writes under.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct C4Home {
    root: PathBuf,
}

impl C4Home {
    /// Resolve from the process environment.
    #[must_use]
    pub fn resolve() -> Self {
        Self::resolve_from(std::env::var_os(HOME_ENV).as_deref(), &user_home())
    }

    /// The resolution rule with its inputs made explicit: the override wins when it is not empty,
    /// else `.c4` under the user's home.
    #[must_use]
    pub fn resolve_from(c4_override: Option<&OsStr>, user_home: &Path) -> Self {
        if let Some(p) = c4_override.filter(|p| !p.is_empty()) {
            return Self::at(p);
        }
        Self::at(user_home.join(".c4"))
    }

    /// A home at a known path.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The root directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `bin/`: the shim copy the installer writes.
    #[must_use]
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// `spool/`: payloads the shim could not deliver, one file each.
    #[must_use]
    pub fn spool_dir(&self) -> PathBuf {
        self.root.join("spool")
    }

    /// `logs/`.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// `install-manifest.json`: what `c4 install` changed, for `c4 uninstall`.
    #[must_use]
    pub fn manifest_path(&self) -> PathBuf {
        self.root.join("install-manifest.json")
    }

    /// `ingest.sock`: the Unix socket the shim writes to.
    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.root.join(SOCKET_NAME)
    }

    /// The Windows pipe name for this home: the prefix plus the FNV-1a hash of the home key.
    #[must_use]
    pub fn pipe_name(&self) -> String {
        format!(
            "{PIPE_PREFIX}{:016x}",
            fnv1a64(home_key(&self.root).as_bytes())
        )
    }

    /// Where the shim connects on this platform: the pipe name on Windows, the socket path elsewhere.
    #[must_use]
    pub fn endpoint(&self) -> String {
        if cfg!(windows) {
            self.pipe_name()
        } else {
            self.socket_path().to_string_lossy().into_owned()
        }
    }
}

/// The user's home directory: `HOME`, else `USERPROFILE`, else the current directory.
#[must_use]
pub fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|p| !p.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|p| !p.is_empty()))
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

/// The home path as the pipe name hashes it: forward slashes, lowercase, no trailing separator, so
/// the spellings Windows treats as one directory give one pipe.
#[must_use]
pub fn home_key(root: &Path) -> String {
    let text = root.to_string_lossy().replace('\\', "/").to_lowercase();
    text.trim_end_matches('/').to_string()
}

/// FNV-1a, 64 bits.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_order() {
        let home = Path::new("/home/example");
        let a = C4Home::resolve_from(Some(OsStr::new("/tmp/c4-scratch")), home);
        assert_eq!(a.root(), Path::new("/tmp/c4-scratch"));
        let b = C4Home::resolve_from(Some(OsStr::new("")), home);
        assert_eq!(b.root(), Path::new("/home/example/.c4"));
        let c = C4Home::resolve_from(None, home);
        assert_eq!(c.root(), Path::new("/home/example/.c4"));
        assert_eq!(c.bin_dir(), Path::new("/home/example/.c4/bin"));
        assert_eq!(c.spool_dir(), Path::new("/home/example/.c4/spool"));
        assert_eq!(
            c.manifest_path(),
            Path::new("/home/example/.c4/install-manifest.json")
        );
        assert_eq!(c.socket_path(), Path::new("/home/example/.c4/ingest.sock"));
    }

    #[test]
    fn fnv1a64_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn one_pipe_per_home_whatever_the_spelling() {
        let a = C4Home::at(r"C:\Users\Example\.c4");
        let b = C4Home::at("c:/users/example/.c4/");
        let c = C4Home::at("/home/example/.c4");
        assert_eq!(home_key(a.root()), "c:/users/example/.c4");
        assert_eq!(a.pipe_name(), b.pipe_name());
        assert_ne!(a.pipe_name(), c.pipe_name());
        assert!(a.pipe_name().starts_with(PIPE_PREFIX));
        assert_eq!(a.pipe_name().len(), PIPE_PREFIX.len() + 16);
        if cfg!(windows) {
            assert_eq!(c.endpoint(), c.pipe_name());
        } else {
            assert_eq!(c.endpoint(), "/home/example/.c4/ingest.sock");
        }
    }
}
