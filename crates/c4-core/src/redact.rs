// <wtl> project: c4 | file: crates/c4-core/src/redact.rs | author: watchthelight </wtl>
//! Redaction for logs, exports and config excerpts. The rules are deliberately broad: a false
//! positive hides a harmless string, a false negative leaks a token.

use regex::Regex;
use std::sync::LazyLock;

static BEARER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{16,}").unwrap());
static ANTHROPIC_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsk-(?:ant-)?[A-Za-z0-9_-]{16,}").unwrap());
static GITHUB_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:gh[pousr]_[A-Za-z0-9]{16,}|github_pat_[A-Za-z0-9_]{16,})").unwrap()
});
static SLACK_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bxox[bpa]-[A-Za-z0-9-]{10,}").unwrap());
static AWS_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bAKIA[0-9A-Z]{16}\b").unwrap());
static JSON_SECRET_FIELD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)("(?:[a-z_]*token|rvAuth|ptyAuth|authToken|accessToken|refreshToken|apiKey|api_key|secret|password|authorization)"\s*:\s*")[^"]*(")"#).unwrap()
});
static ENV_SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b([A-Z0-9_]*(?:TOKEN|SECRET|PASSWORD|API_KEY|APIKEY)[A-Z0-9_]*=)[^\s;&]+")
        .unwrap()
});
static WINDOWS_HOME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[A-Za-z]:[\\/]+Users[\\/]+[^\\/\s\x22']+").unwrap());
static UNIX_HOME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:/Users|/home)/[^/\s\x22']+").unwrap());

/// Replace tokens, keys, secret-looking JSON and env values, and home directories.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut out = BEARER.replace_all(text, "bearer <redacted>").into_owned();
    out = ANTHROPIC_KEY
        .replace_all(&out, "sk-<redacted>")
        .into_owned();
    out = GITHUB_TOKEN.replace_all(&out, "gh_<redacted>").into_owned();
    out = SLACK_TOKEN.replace_all(&out, "xox-<redacted>").into_owned();
    out = AWS_KEY.replace_all(&out, "AKIA<redacted>").into_owned();
    out = JSON_SECRET_FIELD
        .replace_all(&out, "${1}<redacted>${2}")
        .into_owned();
    out = ENV_SECRET.replace_all(&out, "${1}<redacted>").into_owned();
    out = WINDOWS_HOME.replace_all(&out, "~").into_owned();
    out = UNIX_HOME.replace_all(&out, "~").into_owned();
    out
}

/// True when a configuration key is one whose value must never be shown.
#[must_use]
pub fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "AUTHORIZATION",
        "COOKIE",
        "API_KEY",
        "APIKEY",
        "PRIVATE_KEY",
    ]
    .iter()
    .any(|needle| upper.contains(needle))
        || upper == "KEY"
        || upper.ends_with("_KEY")
}

/// True when a value looks like a credential by shape alone.
#[must_use]
pub fn looks_like_secret(value: &str) -> bool {
    let v = value.trim();
    ANTHROPIC_KEY.is_match(v)
        || GITHUB_TOKEN.is_match(v)
        || SLACK_TOKEN.is_match(v)
        || AWS_KEY.is_match(v)
        || (v.len() >= 32
            && !v.contains(' ')
            && v.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.=+/".contains(c)))
}

/// How a redacted value is described in the UI: its length, never its content.
#[must_use]
pub fn describe_secret(value: &str) -> String {
    format!("redacted ({} chars)", value.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_and_keys() {
        // Made-up shapes for the test.
        let s = "Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123 and sk-ant-oat01-abcdefghijklmnopqrstuvwxyz"; // scan:allow
        let r = redact(s);
        assert!(!r.contains("abcdefghijklmnopqrstuvwxyz0123"));
        assert!(r.contains("bearer <redacted>"));
        assert!(r.contains("sk-<redacted>"));
        assert_eq!(
            redact("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ012345"), // scan:allow
            "gh_<redacted>"
        );
    }

    #[test]
    fn json_and_env_fields() {
        let r = redact(r#"{"rvAuth":"abc123","cwd":"/home/example/x","messagingToken":"zzz"}"#); // scan:allow
        assert_eq!(
            r,
            r#"{"rvAuth":"<redacted>","cwd":"~/x","messagingToken":"<redacted>"}"# // scan:allow
        );
        assert_eq!(
            redact("CLAUDE_CODE_MESSAGING_TOKEN=deadbeef PATH=/x"),
            "CLAUDE_CODE_MESSAGING_TOKEN=<redacted> PATH=/x"
        );
    }

    #[test]
    fn home_paths() {
        assert_eq!(redact(r"C:\Users\example\src\c4"), r"~\src\c4");
        assert_eq!(redact("/Users/example/src"), "~/src");
        assert_eq!(
            redact("/home/example/.claude/projects"),
            "~/.claude/projects"
        );
        // Broad on purpose: a /Users/<name> segment goes wherever it sits.
        assert_eq!(redact("/opt/tool/Users/example/x"), "/opt/tool~/x");
    }

    #[test]
    fn secret_names_and_shapes() {
        assert!(is_secret_name("GITHUB_TOKEN"));
        assert!(is_secret_name("Authorization"));
        assert!(is_secret_name("api_key"));
        assert!(!is_secret_name("MCP_TIMEOUT"));
        assert!(!is_secret_name("KEYBOARD_LAYOUT"));
        assert!(looks_like_secret("sk-ant-api03-0123456789abcdefghij")); // scan:allow
        assert!(looks_like_secret("0123456789abcdef0123456789abcdef"));
        assert!(!looks_like_secret("hello world"));
        assert_eq!(describe_secret("abcd"), "redacted (4 chars)");
    }
}
