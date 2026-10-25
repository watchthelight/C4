// <wtl> project: c4 | file: crates/c4-discovery/src/host.rs | author: watchthelight </wtl>
//! Which terminal a Claude Code process runs in, read from the variables terminals put in the
//! environment of the programs they start. The closest host wins: a tmux pane inside a `WezTerm` tab is
//! a tmux pane, because that is where `send-keys` has to go.

use c4_core::HostKind;

/// The only environment variables the process probe keeps. Everything else in a foreign process's
/// environment is dropped at the probe boundary, so a key or token never reaches C4's memory.
pub const HOST_ENV_KEYS: &[&str] = &[
    "TMUX",
    "TMUX_PANE",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "ITERM_SESSION_ID",
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "WT_SESSION",
    "WT_PROFILE_ID",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "VSCODE_PID",
    "VSCODE_INJECTION",
    "ALACRITTY_WINDOW_ID",
    "ALACRITTY_SOCKET",
    "VTE_VERSION",
    "GNOME_TERMINAL_SCREEN",
    "KONSOLE_VERSION",
    "SESSIONNAME",
    "ComSpec",
    "TERM",
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
];

/// True for a variable the probe keeps.
#[must_use]
pub fn keeps_env_key(key: &str) -> bool {
    HOST_ENV_KEYS.iter().any(|k| k.eq_ignore_ascii_case(key))
}

/// A host and, when the host names panes, the pane the process lives in.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct HostHint {
    /// The host.
    pub host: HostKind,
    /// The pane id in the host's own terms: a `WezTerm` pane id, a tmux pane (`%3`), an iTerm2
    /// session id, a kitty window id, a Windows Terminal session guid.
    pub pane: Option<String>,
}

impl HostHint {
    fn new(host: HostKind, pane: Option<&str>) -> Self {
        Self {
            host,
            pane: pane
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string),
        }
    }
}

/// Decide the host from environment pairs.
#[must_use]
pub fn identify_host(env: &[(String, String)]) -> HostHint {
    let get = |key: &str| {
        env.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.trim().is_empty())
    };
    if let Some(pane) = get("TMUX_PANE") {
        return HostHint::new(HostKind::Tmux, Some(pane));
    }
    if get("TMUX").is_some() {
        return HostHint::new(HostKind::Tmux, None);
    }
    if let Some(pane) = get("WEZTERM_PANE") {
        return HostHint::new(HostKind::WezTerm, Some(pane));
    }
    if let Some(id) = get("ITERM_SESSION_ID") {
        return HostHint::new(HostKind::ITerm2, Some(id));
    }
    if let Some(id) = get("KITTY_WINDOW_ID") {
        return HostHint::new(HostKind::Kitty, Some(id));
    }
    if let Some(id) = get("WT_SESSION") {
        return HostHint::new(HostKind::WindowsTerminal, Some(id));
    }
    let term_program = get("TERM_PROGRAM").map(str::to_ascii_lowercase);
    if get("VSCODE_PID").is_some()
        || get("VSCODE_INJECTION").is_some()
        || term_program.as_deref() == Some("vscode")
    {
        return HostHint::new(HostKind::VsCode, None);
    }
    if let Some(id) = get("ALACRITTY_WINDOW_ID") {
        return HostHint::new(HostKind::Alacritty, Some(id));
    }
    match term_program.as_deref() {
        Some("wezterm") => return HostHint::new(HostKind::WezTerm, None),
        Some("iterm.app") => return HostHint::new(HostKind::ITerm2, None),
        Some("apple_terminal") => {
            return HostHint::new(HostKind::TerminalApp, get("TERM_SESSION_ID"));
        }
        Some("alacritty") => return HostHint::new(HostKind::Alacritty, None),
        Some("tmux") => return HostHint::new(HostKind::Tmux, None),
        _ => {}
    }
    if get("GNOME_TERMINAL_SCREEN").is_some() || get("VTE_VERSION").is_some() {
        return HostHint::new(HostKind::GnomeTerminal, None);
    }
    if get("ComSpec").is_some() && get("SESSIONNAME").is_some() {
        // A Windows process with no terminal variables: a plain console window.
        return HostHint::new(HostKind::Conhost, None);
    }
    HostHint::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn closest_host_wins() {
        let h = identify_host(&env(&[
            ("WEZTERM_PANE", "4"),
            ("TMUX_PANE", "%2"),
            ("TMUX", "/tmp/tmux-1/default,1,0"),
        ]));
        assert_eq!(h, HostHint::new(HostKind::Tmux, Some("%2")));
        let h = identify_host(&env(&[("WEZTERM_PANE", "4"), ("TERM_PROGRAM", "WezTerm")]));
        assert_eq!(h, HostHint::new(HostKind::WezTerm, Some("4")));
        let h = identify_host(&env(&[
            ("WT_SESSION", "0000-1"),
            ("ComSpec", "cmd.exe"),
            ("SESSIONNAME", "Console"),
        ]));
        assert_eq!(h, HostHint::new(HostKind::WindowsTerminal, Some("0000-1")));
        let h = identify_host(&env(&[("ComSpec", "cmd.exe"), ("SESSIONNAME", "Console")]));
        assert_eq!(h.host, HostKind::Conhost);
        let h = identify_host(&env(&[("TERM_PROGRAM", "vscode"), ("WT_SESSION", "x")]));
        assert_eq!(
            h.host,
            HostKind::WindowsTerminal,
            "the outer terminal wins over an IDE marker"
        );
        let h = identify_host(&env(&[("TERM_PROGRAM", "vscode")]));
        assert_eq!(h.host, HostKind::VsCode);
        let h = identify_host(&env(&[
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM_SESSION_ID", "w0t0p0:1"),
        ]));
        assert_eq!(h, HostHint::new(HostKind::TerminalApp, Some("w0t0p0:1")));
        let h = identify_host(&env(&[("ITERM_SESSION_ID", "w0t1p0:ABCD")]));
        assert_eq!(h, HostHint::new(HostKind::ITerm2, Some("w0t1p0:ABCD")));
        let h = identify_host(&env(&[("VTE_VERSION", "7600")]));
        assert_eq!(h.host, HostKind::GnomeTerminal);
        assert_eq!(
            identify_host(&env(&[("TERM", "xterm")])),
            HostHint::default()
        );
        assert_eq!(
            identify_host(&env(&[("WEZTERM_PANE", "  ")])).host,
            HostKind::Unknown
        );
    }

    #[test]
    fn keeps_only_host_keys() {
        assert!(keeps_env_key("WT_SESSION"));
        assert!(keeps_env_key("comspec"));
        assert!(!keeps_env_key("ANTHROPIC_API_KEY"));
        assert!(!keeps_env_key("PATH"));
    }
}
