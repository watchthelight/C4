// <wtl> project: c4 | file: crates/c4-testkit/tests/discovery.rs | author: watchthelight </wtl>
//! Tests A and B of the directive, headless: sessions that were running before discovery started
//! are listed, and a session started afterwards shows up within two seconds. Both run against real
//! child processes and the real process table, never against `~/.claude`.

use c4_core::{
    Attachment, DisconnectReason, HostKind, SessionId, SessionState, SessionView, StateKind,
    Timestamp,
};
use c4_discovery::{
    Discovery, FakeProbe, Liveness, ProcessProbe, SysinfoProbe, check_liveness, scan_registry,
};
use c4_testkit::{FAKE_VERSION, FakeClaude, FakeClaudeOptions, ScratchHome};
use std::time::{Duration, Instant};

fn view(disc: &Discovery, id: &SessionId) -> SessionView {
    disc.table()
        .get(id)
        .unwrap_or_else(|| panic!("no view for {id}"))
        .clone()
}

#[test]
fn sessions_running_before_discovery_starts_are_listed() {
    let scratch = ScratchHome::empty().unwrap();
    let home = scratch.home();
    let alpha = FakeClaude::start(
        home,
        FakeClaudeOptions::new(scratch.project_dir("alpha").unwrap())
            .status("busy", None)
            .name("alpha build", "user"),
    )
    .unwrap();
    let beta = FakeClaude::start(
        home,
        FakeClaudeOptions::new(scratch.project_dir("beta").unwrap())
            .status("waiting", Some("permission prompt")),
    )
    .unwrap();
    let gamma = FakeClaude::start(
        home,
        FakeClaudeOptions::new(scratch.project_dir("gamma").unwrap())
            .status("idle", None)
            .env("WEZTERM_PANE", "7"),
    )
    .unwrap();
    // Only alpha has written a transcript so far.
    alpha
        .append_transcript(r#"{"type":"user","uuid":"u1","message":{"content":"hello"}}"#)
        .unwrap();

    let mut disc = Discovery::new(scratch.files_only());
    let mut probe = SysinfoProbe::new();
    let started = Instant::now();
    let res = disc.refresh(&mut probe, Timestamp::now());
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "first list took {elapsed:?}"
    );
    assert_eq!(res.report.registry_entries, 3);
    assert_eq!(res.report.registry_skipped, 0);
    assert_eq!(res.report.probed, 3);
    assert_eq!(res.report.alive, 3);
    assert_eq!(res.report.transcripts, 1, "alpha's transcript exists");
    assert_eq!(res.report.problems, Vec::new());
    assert_eq!(disc.table().len(), 3);

    let (alpha_id, beta_id, gamma_id) = (
        alpha.session().clone(),
        beta.session().clone(),
        gamma.session().clone(),
    );
    let a = view(&disc, &alpha_id);
    assert!(a.alive);
    assert_eq!(a.state, SessionState::Working);
    assert_eq!(a.process, Some(alpha.process()));
    assert_eq!(a.display_name(), "alpha build");
    assert_eq!(a.name_source.as_deref(), Some("user"));
    assert_eq!(a.cwd.as_deref(), Some(alpha.cwd()));
    assert_eq!(a.version.as_deref(), Some(FAKE_VERSION));
    assert_eq!(a.attachment, Attachment::Observed);
    assert_eq!(a.host, HostKind::Unknown, "host variables are stripped");
    assert!(a.started_at.is_some());
    assert!(a.evidence.is_observed());
    assert_eq!(
        a.transcript_path.as_deref(),
        Some(alpha.transcript_path().display().to_string().as_str()),
        "the transcript alpha wrote is found by its derived path"
    );

    let b = view(&disc, &beta_id);
    assert_eq!(b.state.kind(), StateKind::WaitingPermission);
    assert!(b.waiting_since.is_some());
    assert_eq!(b.transcript_path, None, "beta has not written one");
    assert_eq!(
        b.display_name(),
        "beta",
        "the cwd tail when there is no name"
    );

    let g = view(&disc, &gamma_id);
    assert_eq!(g.state, SessionState::Idle);
    assert_eq!(g.host, HostKind::WezTerm);
    assert_eq!(g.pane.as_deref(), Some("7"));

    let order: Vec<SessionId> = disc.views().iter().map(|v| v.id.clone()).collect();
    assert_eq!(order[0], beta_id, "the waiting session sorts first");
    assert_eq!(order[1], alpha_id);
    assert_eq!(order[2], gamma_id);

    // A clean exit removes the record; a crash leaves it. Both end as Disconnected, never as
    // Completed, because nothing said the sessions finished.
    alpha.exit();
    gamma.crash().unwrap();
    let res = disc.refresh(&mut probe, Timestamp::now());
    assert_eq!(res.report.registry_entries, 2, "beta and the crashed gamma");
    assert_eq!(
        res.report.probed, 3,
        "alpha is still probed until it is gone"
    );
    assert_eq!(res.report.alive, 1);
    let a = view(&disc, &alpha_id);
    assert!(!a.alive);
    assert_eq!(a.state.kind(), StateKind::Disconnected);
    assert_eq!(a.attachment, Attachment::Disconnected);
    let g = view(&disc, &gamma_id);
    assert!(!g.alive);
    assert_eq!(
        g.state,
        SessionState::Disconnected {
            reason: DisconnectReason::ProcessGone
        }
    );
    assert_eq!(
        view(&disc, &beta_id).state.kind(),
        StateKind::WaitingPermission
    );
    assert_eq!(disc.table().len(), 3, "identities are kept");

    let res = disc.refresh(&mut probe, Timestamp::now());
    assert_eq!(res.report.probed, 2, "a dead process is not probed again");
}

#[test]
fn a_session_started_after_discovery_appears_within_two_seconds() {
    let scratch = ScratchHome::empty().unwrap();
    let mut disc = Discovery::new(scratch.files_only());
    let mut probe = SysinfoProbe::new();
    disc.refresh(&mut probe, Timestamp::now());
    assert!(disc.table().is_empty(), "nothing registered yet");

    let late = FakeClaude::start(
        scratch.home(),
        FakeClaudeOptions::new(scratch.project_dir("late").unwrap()).status("busy", None),
    )
    .unwrap();
    let started = Instant::now();
    let mut seen_after = None;
    while started.elapsed() < Duration::from_secs(2) {
        let res = disc.refresh(&mut probe, Timestamp::now());
        if disc.table().get(late.session()).is_some_and(|v| v.alive) {
            assert!(
                res.transitions
                    .iter()
                    .any(|(id, t)| id == late.session() && t.to == SessionState::Working),
                "the first sight carries the transition: {:?}",
                res.transitions
            );
            seen_after = Some(started.elapsed());
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let seen_after = seen_after.expect("the late session within 2 s");
    assert!(seen_after < Duration::from_secs(2), "{seen_after:?}");

    // A status change on the record shows on the next refresh.
    let mut late = late;
    late.set_status("waiting", Some("input needed")).unwrap();
    disc.refresh(&mut probe, Timestamp::now());
    let v = disc.table().get(late.session()).unwrap();
    assert_eq!(v.state.kind(), StateKind::WaitingUserInput);
    late.set_name("late night", "auto").unwrap();
    disc.refresh(&mut probe, Timestamp::now());
    assert_eq!(
        disc.table().get(late.session()).unwrap().display_name(),
        "late night"
    );
}

#[test]
fn the_stand_in_passes_the_liveness_ladder_and_cleans_up_after_itself() {
    let scratch = ScratchHome::empty().unwrap();
    let fake = FakeClaude::start(
        scratch.home(),
        FakeClaudeOptions::new(scratch.project_dir("p").unwrap()),
    )
    .unwrap();
    let pid = fake.pid();
    let record = fake.record_path().to_path_buf();
    assert!(record.starts_with(scratch.root()));
    assert_eq!(
        fake.transcript_path(),
        scratch.home().transcript_path(fake.cwd(), fake.session())
    );

    let mut probe = SysinfoProbe::new();
    let info = probe.probe(pid).expect("the child is in the process table");
    assert_eq!(
        check_liveness(Some(&info), &fake.process()),
        Liveness::Alive
    );
    // cmd.exe sets ComSpec in its own environment; nothing else may be there.
    assert!(
        info.env
            .iter()
            .all(|(k, _)| k.eq_ignore_ascii_case("ComSpec")),
        "no host variable reaches the child: {:?}",
        info.env
    );

    let scan = scan_registry(&scratch.home().sessions_dir());
    assert_eq!(scan.entries.len(), 1);
    assert_eq!(scan.entries[0].pid, pid);
    assert_eq!(scan.entries[0].process(), fake.process());
    assert_eq!(scan.entries[0].session(), *fake.session());
    assert_eq!(scan.entries[0].version.as_deref(), Some(FAKE_VERSION));
    assert_eq!(scan.skipped, Vec::<std::path::PathBuf>::new());

    drop(fake);
    assert!(!record.exists(), "a dropped stand-in removes its record");
    let gone = Instant::now() + Duration::from_secs(5);
    while Instant::now() < gone && probe.probe(pid).is_some() {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(probe.probe(pid).is_none(), "the child is gone");
}

#[test]
fn a_seeded_home_folds_with_every_fixture_pid_dead() {
    let scratch = ScratchHome::seeded().unwrap();
    let mut disc = Discovery::new(scratch.files_only());
    let res = disc.refresh(&mut FakeProbe::new(), Timestamp::now());
    assert_eq!(res.report.registry_entries, 4);
    assert_eq!(res.report.registry_skipped, 2, "the key file and notes.txt");
    assert_eq!(res.report.roster_workers, 2);
    assert_eq!(res.report.jobs, 2);
    assert_eq!(res.report.alive, 0);
    assert!(
        disc.views()
            .iter()
            .all(|v| v.state.kind() == StateKind::Disconnected)
    );
}
