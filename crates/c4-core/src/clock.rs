// <wtl> project: c4 | file: crates/c4-core/src/clock.rs | author: watchthelight </wtl>
//! A millisecond timestamp that serializes as a plain integer and parses the RFC 3339 strings found
//! in transcripts and hook payloads.

use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// Milliseconds since the Unix epoch, UTC.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    /// The current time.
    #[must_use]
    pub fn now() -> Self {
        let nanos = OffsetDateTime::now_utc().unix_timestamp_nanos();
        Self((nanos / 1_000_000) as i64)
    }

    /// From milliseconds since the epoch.
    #[must_use]
    pub fn from_millis(ms: i64) -> Self {
        Self(ms)
    }

    /// From whole seconds since the epoch.
    #[must_use]
    pub fn from_secs(secs: i64) -> Self {
        Self(secs.saturating_mul(1000))
    }

    /// From a [`std::time::SystemTime`], such as a file's modification time. `None` before 1970.
    #[must_use]
    pub fn from_system_time(t: std::time::SystemTime) -> Option<Self> {
        let since_epoch = t.duration_since(std::time::UNIX_EPOCH).ok()?;
        i64::try_from(since_epoch.as_millis()).ok().map(Self)
    }

    /// Parse an RFC 3339 string such as `2026-10-25T18:07:12.345Z`.
    #[must_use]
    pub fn parse_rfc3339(s: &str) -> Option<Self> {
        let dt = OffsetDateTime::parse(s.trim(), &Rfc3339).ok()?;
        Some(Self((dt.unix_timestamp_nanos() / 1_000_000) as i64))
    }

    /// Format as RFC 3339 in UTC, with milliseconds when they are not zero.
    #[must_use]
    pub fn to_rfc3339(self) -> String {
        let nanos = i128::from(self.0) * 1_000_000;
        OffsetDateTime::from_unix_timestamp_nanos(nanos)
            .ok()
            .and_then(|dt| dt.format(&Rfc3339).ok())
            .unwrap_or_else(|| self.0.to_string())
    }

    /// Format for a file name: `20261026T051530Z`, UTC to the second, nothing a file system minds.
    #[must_use]
    pub fn to_file_stamp(self) -> String {
        let format =
            time::macros::format_description!("[year][month][day]T[hour][minute][second]Z");
        let nanos = i128::from(self.0) * 1_000_000;
        OffsetDateTime::from_unix_timestamp_nanos(nanos)
            .ok()
            .and_then(|dt| dt.format(&format).ok())
            .unwrap_or_else(|| self.0.to_string())
    }

    /// Milliseconds from `earlier` to `self`; negative when `earlier` is later.
    #[must_use]
    pub fn millis_since(self, earlier: Timestamp) -> i64 {
        self.0.saturating_sub(earlier.0)
    }

    /// This timestamp moved forward by `ms` milliseconds.
    #[must_use]
    pub fn plus_millis(self, ms: i64) -> Self {
        Self(self.0.saturating_add(ms))
    }

    /// Milliseconds since the epoch.
    #[must_use]
    pub fn millis(self) -> i64 {
        self.0
    }
}

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Timestamp({})", self.to_rfc3339())
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_rfc3339())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats_rfc3339() {
        let t = Timestamp::parse_rfc3339("2026-10-25T18:07:12.345Z").unwrap();
        assert_eq!(t.0, 1_792_951_632_345);
        assert_eq!(t.to_rfc3339(), "2026-10-25T18:07:12.345Z");
        assert_eq!(Timestamp::from_secs(0).to_rfc3339(), "1970-01-01T00:00:00Z");
        assert!(Timestamp::parse_rfc3339("yesterday").is_none());
        assert_eq!(t.to_file_stamp(), "20261025T180712Z");
    }

    #[test]
    fn arithmetic() {
        let a = Timestamp::from_millis(1000);
        let b = a.plus_millis(500);
        assert_eq!(b.millis_since(a), 500);
        assert_eq!(a.millis_since(b), -500);
        assert_eq!(Timestamp::from_secs(2).0, 2000);
    }

    #[test]
    fn from_system_time_is_millis() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_millis(1500);
        assert_eq!(
            Timestamp::from_system_time(t),
            Some(Timestamp::from_millis(1500))
        );
        let before_epoch = std::time::UNIX_EPOCH - std::time::Duration::from_secs(1);
        assert_eq!(Timestamp::from_system_time(before_epoch), None);
    }

    #[test]
    fn serializes_as_integer() {
        let t = Timestamp::from_millis(42);
        assert_eq!(serde_json::to_string(&t).unwrap(), "42");
        let back: Timestamp = serde_json::from_str("42").unwrap();
        assert_eq!(back, t);
    }
}
