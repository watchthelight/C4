// <wtl> project: c4 | file: crates/c4-discovery/examples/live_probe.rs | author: watchthelight </wtl>
//! Read-only dry run of the discovery readers against the real Claude home. Prints the session
//! table with secrets already stripped and home paths redacted; nothing is written anywhere.
//!
//! `cargo run -p c4-discovery --example live_probe`

use c4_core::{Timestamp, redact::redact};
use c4_discovery::{Discovery, DiscoveryOptions, SysinfoProbe};

fn main() {
    let opts = DiscoveryOptions::from_env();
    println!("home: {}", redact(&opts.home.root().display().to_string()));
    let mut disc = Discovery::new(opts);
    let mut probe = SysinfoProbe::new();
    let res = disc.refresh(&mut probe, Timestamp::now());
    let report = serde_json::to_string_pretty(&res.report).unwrap_or_default();
    println!("{}", redact(&report));
    for view in disc.views() {
        println!(
            "{} {:<14} {:<10} {:<22} {:<16} alive={} pane={:?} name={} cwd={}",
            view.id.short(),
            format!("{:?}", view.session_kind),
            view.host.label(),
            view.state.describe(),
            format!("{:?}", view.attachment),
            view.alive,
            view.pane,
            view.display_name(),
            redact(view.cwd.as_deref().unwrap_or("-")),
        );
    }
    for (id, t) in &res.transitions {
        println!(
            "transition {} {:?} -> {:?}",
            id.short(),
            t.from.kind(),
            t.to.kind()
        );
    }
}
