// <wtl> project: c4 | file: crates/c4-core/tests/labels.rs | author: watchthelight </wtl>
//! `fixtures/labels.json` is the contract between the core and the window: the label, weight and
//! attention rule of every state, the labels of the reasons, hosts, attachments and sources, and
//! what `describe` says for a set of states. This checks the Rust side against the file;
//! `apps/desktop/tests/labels.test.ts` checks the TypeScript side against the same file.

use c4_core::{
    Attachment, DisconnectReason, HostKind, InputReason, SessionState, Source, StateKind,
};
use serde::Serialize;
use serde_json::Value;
use std::fmt::Debug;

const CONTRACT: &str = include_str!("../../../fixtures/labels.json");

fn contract() -> Value {
    serde_json::from_str(CONTRACT).expect("fixtures/labels.json parses")
}

/// The JSON key of an enum variant: its serde name.
fn key<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .expect("serializes")
        .as_str()
        .expect("a unit variant serializes as a string")
        .to_string()
}

/// Every variant in `all` has an entry whose value is its label, and the map holds nothing else.
fn check<T: Serialize + Copy + Debug>(map: &Value, all: &[T], label: impl Fn(T) -> &'static str) {
    let map = map.as_object().expect("an object");
    assert_eq!(
        map.len(),
        all.len(),
        "every variant has an entry and nothing else does"
    );
    for variant in all {
        assert_eq!(map[&key(variant)], label(*variant), "{variant:?}");
    }
}

#[test]
fn states_carry_label_weight_attention_and_liveness() {
    let c = contract();
    let states = c["states"].as_object().expect("an object");
    assert_eq!(states.len(), StateKind::ALL.len());
    for kind in StateKind::ALL {
        let entry = &states[&key(&kind)];
        assert_eq!(entry["label"], kind.label(), "{kind:?}");
        assert_eq!(entry["weight"], kind.sort_weight(), "{kind:?}");
        assert_eq!(entry["attention"], kind.needs_attention(), "{kind:?}");
        assert_eq!(entry["live"], kind.is_live(), "{kind:?}");
    }
}

#[test]
fn reasons_hosts_and_attachments_have_their_labels() {
    const INPUT: [InputReason; 6] = [
        InputReason::Question,
        InputReason::Elicitation,
        InputReason::AgentNeedsInput,
        InputReason::Auth,
        InputReason::Dialog,
        InputReason::Other,
    ];
    const DISCONNECT: [DisconnectReason; 5] = [
        DisconnectReason::ProcessGone,
        DisconnectReason::PidReused,
        DisconnectReason::RegistryGone,
        DisconnectReason::HostGone,
        DisconnectReason::HostExited,
    ];
    const HOSTS: [HostKind; 12] = [
        HostKind::C4,
        HostKind::WezTerm,
        HostKind::Tmux,
        HostKind::ITerm2,
        HostKind::Kitty,
        HostKind::WindowsTerminal,
        HostKind::TerminalApp,
        HostKind::VsCode,
        HostKind::GnomeTerminal,
        HostKind::Alacritty,
        HostKind::Conhost,
        HostKind::Unknown,
    ];
    const ATTACHMENTS: [Attachment; 4] = [
        Attachment::Managed,
        Attachment::InteractiveAttached,
        Attachment::Observed,
        Attachment::Disconnected,
    ];
    let c = contract();
    check(&c["input_reasons"], &INPUT, InputReason::label);
    check(
        &c["disconnect_reasons"],
        &DISCONNECT,
        DisconnectReason::label,
    );
    check(&c["hosts"], &HOSTS, HostKind::label);
    check(&c["attachments"], &ATTACHMENTS, Attachment::label);
}

#[test]
fn describe_and_source_labels_match_the_cases() {
    let c = contract();
    let cases = c["describe"].as_array().expect("an array");
    assert!(cases.len() >= 12);
    for case in cases {
        let state: SessionState =
            serde_json::from_value(case["state"].clone()).unwrap_or_else(|e| panic!("{e}: {case}"));
        assert_eq!(state.describe(), case["text"], "{case}");
    }
    let sources = c["sources"].as_array().expect("an array");
    assert_eq!(sources.len(), 12, "one case per source variant");
    for case in sources {
        let source: Source = serde_json::from_value(case["source"].clone())
            .unwrap_or_else(|e| panic!("{e}: {case}"));
        assert_eq!(source.label(), case["label"], "{case}");
    }
}
