// <wtl> project: c4 | file: crates/c4-core/src/table.rs | author: watchthelight </wtl>
//! Every session C4 knows about, keyed by session id, with the attention order the sidebar uses.

use crate::clock::Timestamp;
use crate::ids::SessionId;
use crate::observation::Observation;
use crate::reduce::{Transition, reduce, tick};
use crate::view::SessionView;
use std::collections::BTreeMap;

/// The set of known sessions.
#[derive(Clone, Debug, Default)]
pub struct SessionTable {
    views: BTreeMap<SessionId, SessionView>,
}

impl SessionTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold an observation into its session, creating the view the first time the id is seen.
    pub fn apply(&mut self, obs: &Observation, now: Timestamp) -> Option<Transition> {
        let view = self
            .views
            .entry(obs.session.clone())
            .or_insert_with(|| SessionView::new(obs.session.clone(), now));
        reduce(view, obs)
    }

    /// Run the clock over every session. Returns the ids whose view changed.
    pub fn tick(&mut self, now: Timestamp) -> Vec<SessionId> {
        self.views
            .iter_mut()
            .filter_map(|(id, view)| tick(view, now).then(|| id.clone()))
            .collect()
    }

    /// One session.
    #[must_use]
    pub fn get(&self, id: &SessionId) -> Option<&SessionView> {
        self.views.get(id)
    }

    /// One session, mutable.
    pub fn get_mut(&mut self, id: &SessionId) -> Option<&mut SessionView> {
        self.views.get_mut(id)
    }

    /// Forget a session.
    pub fn remove(&mut self, id: &SessionId) -> Option<SessionView> {
        self.views.remove(id)
    }

    /// Every session, in id order.
    pub fn iter(&self) -> impl Iterator<Item = &SessionView> {
        self.views.values()
    }

    /// Every id, in id order.
    pub fn ids(&self) -> impl Iterator<Item = &SessionId> {
        self.views.keys()
    }

    /// How many sessions are known.
    #[must_use]
    pub fn len(&self) -> usize {
        self.views.len()
    }

    /// True when nothing is known.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }

    /// Every session in attention order: the states that need the user first, then the longest
    /// wait, then the most recent activity.
    #[must_use]
    pub fn sorted(&self) -> Vec<&SessionView> {
        let mut all: Vec<&SessionView> = self.views.values().collect();
        all.sort_by_key(|view| view.sort_key());
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{ProcessRef, SessionKind};
    use crate::observation::{BgState, HookEvent, ObservationBody, RegistryStatus};
    use crate::state::StateKind;

    fn registry(
        id: &str,
        status: RegistryStatus,
        waiting_for: Option<&str>,
        at: i64,
    ) -> Observation {
        Observation::new(
            SessionId::new(id),
            Timestamp::from_millis(at),
            ObservationBody::Registry {
                process: ProcessRef::new(1, 1),
                session_kind: SessionKind::Interactive,
                cwd: Some("/home/example/x".into()),
                name: None,
                name_source: None,
                status,
                waiting_for: waiting_for.map(str::to_string),
                version: None,
                socket: None,
                started_at: None,
            },
        )
    }

    #[test]
    fn creates_views_and_sorts_by_attention() {
        let mut table = SessionTable::new();
        let now = Timestamp::from_millis(100);
        table.apply(&registry("b", RegistryStatus::Busy, None, 100), now);
        table.apply(&registry("a", RegistryStatus::Idle, None, 100), now);
        table.apply(
            &registry("c", RegistryStatus::Waiting, Some("approve Edit"), 100),
            now,
        );
        assert_eq!(table.len(), 3);
        let order: Vec<&str> = table.sorted().iter().map(|v| v.id.as_str()).collect();
        assert_eq!(order, ["c", "b", "a"]);
        assert_eq!(table.get(&SessionId::new("c")).unwrap().first_seen, now);
        assert!(table.remove(&SessionId::new("a")).is_some());
        assert_eq!(table.ids().count(), 2);
    }

    #[test]
    fn tick_reports_changed_sessions() {
        let mut table = SessionTable::new();
        let now = Timestamp::from_millis(0);
        let prompt = Observation::new(
            SessionId::new("a"),
            now,
            ObservationBody::Hook(HookEvent::UserPromptSubmit),
        );
        table.apply(&prompt, now);
        assert_eq!(
            table.tick(Timestamp::from_millis(1_000)),
            Vec::<SessionId>::new()
        );
        let later = Timestamp::from_millis(crate::reduce::STALL_AFTER_MS + 1_000);
        assert_eq!(table.tick(later), vec![SessionId::new("a")]);
        assert!(table.get(&SessionId::new("a")).unwrap().stalled);
    }

    #[test]
    fn daemon_observations_describe_background_sessions() {
        let mut table = SessionTable::new();
        let now = Timestamp::from_millis(5_000);
        let daemon = |state, needs: Option<&str>, in_flight| {
            Observation::new(
                SessionId::new("bg-1"),
                now,
                ObservationBody::Daemon {
                    short: "0b2c3d4e".into(),
                    process: Some(ProcessRef::new(41011, 7)),
                    state,
                    tempo: Some("active".into()),
                    detail: Some("running cargo test".into()),
                    needs: needs.map(str::to_string),
                    in_flight,
                    cwd: Some("/home/example/worker".into()),
                    name: Some("worker one".into()),
                    name_source: Some("user".into()),
                    version: Some("2.1.299".into()),
                },
            )
        };
        let t = table
            .apply(&daemon(BgState::Working, Some(""), 1), now)
            .unwrap();
        assert_eq!(t.to.kind(), StateKind::Working);
        let view = table.get(&SessionId::new("bg-1")).unwrap();
        assert_eq!(view.session_kind, SessionKind::Background);
        assert_eq!(view.daemon_short.as_deref(), Some("0b2c3d4e"));
        assert_eq!(view.daemon_in_flight, 1);
        assert_eq!(view.display_name(), "worker one");
        assert_eq!(view.process, Some(ProcessRef::new(41011, 7)));

        let later = now.plus_millis(1_000);
        let blocked = Observation {
            at: later,
            ..daemon(BgState::Blocked, Some("Approve the Bash command"), 0)
        };
        table.apply(&blocked, later);
        let view = table.get(&SessionId::new("bg-1")).unwrap();
        assert_eq!(view.state.kind(), StateKind::WaitingPermission);
        assert_eq!(
            view.state.describe(),
            "waiting for permission: Approve the Bash command"
        );

        let done = Observation {
            at: later.plus_millis(1_000),
            ..daemon(BgState::Done, None, 0)
        };
        table.apply(&done, later);
        assert_eq!(
            table.get(&SessionId::new("bg-1")).unwrap().state.kind(),
            StateKind::Idle
        );
    }
}
