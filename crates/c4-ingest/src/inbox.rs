// <wtl> project: c4 | file: crates/c4-ingest/src/inbox.rs | author: watchthelight </wtl>
//! The queue between the server and whoever folds lines into the session table. Bounded: when it is
//! full the oldest item goes and a counter says so, because a producer (a hook, the status line)
//! must never wait on the app.

use std::collections::VecDeque;
use std::sync::{Mutex, PoisonError};

use tokio::sync::Notify;

use crate::parse::Ingested;

/// Items the inbox holds before it drops the oldest.
pub const DEFAULT_CAPACITY: usize = 4096;

/// What the inbox has seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct InboxStats {
    /// Items waiting right now.
    pub queued: usize,
    /// Items pushed since the start.
    pub received: u64,
    /// Items dropped because the inbox was full.
    pub dropped: u64,
}

struct State {
    queue: VecDeque<Ingested>,
    received: u64,
    dropped: u64,
}

/// The bounded queue. Push from any thread; drain from the consumer; `wait` until there is
/// something to drain.
pub struct Inbox {
    state: Mutex<State>,
    notify: Notify,
    capacity: usize,
}

impl Default for Inbox {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl Inbox {
    /// An inbox that holds at most `capacity` items (at least one).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                received: 0,
                dropped: 0,
            }),
            notify: Notify::new(),
            capacity: capacity.max(1),
        }
    }

    /// Queue one item, dropping the oldest when full.
    pub fn push(&self, item: Ingested) {
        {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.received += 1;
            if state.queue.len() >= self.capacity {
                state.queue.pop_front();
                state.dropped += 1;
            }
            state.queue.push_back(item);
        }
        self.notify.notify_one();
    }

    /// Take everything queued, in the order it arrived. After a spool replay that order is not
    /// the order of `recv_at`; sort by it when that matters.
    pub fn drain(&self) -> Vec<Ingested> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.queue.drain(..).collect()
    }

    /// Items waiting.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .queue
            .len()
    }

    /// Nothing waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return once at least one item waits. A push that lands between the check and the wait is
    /// not lost: the notification is stored until someone waits for it.
    pub async fn wait(&self) {
        loop {
            if !self.is_empty() {
                return;
            }
            self.notify.notified().await;
        }
    }

    /// The counters.
    #[must_use]
    pub fn stats(&self) -> InboxStats {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        InboxStats {
            queued: state.queue.len(),
            received: state.received,
            dropped: state.dropped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_line;

    fn item(n: u32) -> Ingested {
        parse_line(&format!(
            r#"{{"kind":"hook","recv_at":{n},"pid":1,"payload":{{"session_id":"s1","hook_event_name":"UserPromptSubmit"}}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn the_oldest_goes_when_full() {
        let inbox = Inbox::new(2);
        inbox.push(item(1));
        inbox.push(item(2));
        inbox.push(item(3));
        assert_eq!(
            inbox.stats(),
            InboxStats {
                queued: 2,
                received: 3,
                dropped: 1
            }
        );
        let items = inbox.drain();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].recv_at.millis(), 2);
        assert_eq!(items[1].recv_at.millis(), 3);
        assert!(inbox.is_empty());
        assert_eq!(inbox.stats().queued, 0);
    }

    #[tokio::test]
    async fn wait_returns_once_something_is_queued() {
        let inbox = std::sync::Arc::new(Inbox::new(8));
        let waiter = {
            let inbox = inbox.clone();
            tokio::spawn(async move {
                inbox.wait().await;
                inbox.drain().len()
            })
        };
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        inbox.push(item(1));
        assert_eq!(waiter.await.unwrap(), 1);
        // A push before the wait is seen as well.
        inbox.push(item(2));
        tokio::time::timeout(std::time::Duration::from_secs(1), inbox.wait())
            .await
            .expect("an item is already there");
    }
}
