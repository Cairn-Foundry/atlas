// Copyright (C) 2026 Benjamin Bonneton and the Cairn Foundry contributors
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Time as the daemon reads and writes it: ISO 8601 dates in UTC with milliseconds, which sort as text.

use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};

/// Where the daemon reads the time from; tests give their own.
pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

pub fn system_clock() -> Clock {
    Arc::new(Utc::now)
}

pub fn iso(date: DateTime<Utc>) -> String {
    date.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn parse_iso(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.with_timezone(&Utc))
}

/// Milliseconds from the ISO date `since` to `now`, zero when the date is unreadable or in the future.
pub fn elapsed_ms(since: &str, now: DateTime<Utc>) -> i64 {
    parse_iso(since).map_or(0, |start| (now - start).num_milliseconds().max(0))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use chrono::{Duration, TimeZone};

    use super::*;

    /// A clock tests move by hand.
    #[derive(Clone)]
    pub(crate) struct TestClock(Arc<Mutex<DateTime<Utc>>>);

    impl TestClock {
        pub(crate) fn new() -> Self {
            Self(Arc::new(Mutex::new(
                Utc.with_ymd_and_hms(2026, 1, 1, 10, 0, 0).single().expect("date"),
            )))
        }

        pub(crate) fn advance_minutes(&self, minutes: i64) {
            *self.0.lock().expect("clock") += Duration::minutes(minutes);
        }

        pub(crate) fn advance_ms(&self, milliseconds: i64) {
            *self.0.lock().expect("clock") += Duration::milliseconds(milliseconds);
        }

        pub(crate) fn clock(&self) -> Clock {
            let shared = Arc::clone(&self.0);
            Arc::new(move || *shared.lock().expect("clock"))
        }
    }

    #[test]
    fn writes_dates_the_way_the_front_and_the_registry_expect() {
        let clock = TestClock::new();
        assert_eq!(iso((clock.clock())()), "2026-01-01T10:00:00.000Z");
        clock.advance_ms(1500);
        assert_eq!(iso((clock.clock())()), "2026-01-01T10:00:01.500Z");
    }

    #[test]
    fn measures_the_time_since_a_date_and_never_goes_negative() {
        let clock = TestClock::new();
        let start = iso((clock.clock())());
        clock.advance_minutes(2);
        assert_eq!(elapsed_ms(&start, (clock.clock())()), 120_000);
        assert_eq!(elapsed_ms("2999-01-01T00:00:00.000Z", (clock.clock())()), 0);
        assert_eq!(elapsed_ms("not a date", (clock.clock())()), 0);
    }
}
