//! Wall clock and local calendar: the only time-zone-aware code in timewent.

use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{Datelike, Days, LocalResult, NaiveDate, NaiveTime, TimeZone};

/// Unix ms. A clock before 1970 reads as 0.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Seconds east of UTC in the system time zone at `ms` (DST-aware); 0 if unknown.
pub fn local_offset_s(ms: i64) -> i32 {
    chrono::Local
        .timestamp_millis_opt(ms)
        .earliest()
        .map_or(0, |t| t.offset().local_minus_utc())
}

/// The system's IANA time zone name (`Europe/Istanbul`), for the report.
pub fn timezone_name() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "local".into())
}

/// Start of the local day containing `now_ms` ("today" in the ui).
pub fn local_midnight_ms(now_ms: i64) -> i64 {
    day_start_in(&chrono::Local, now_ms)
}

/// Start of the local week (Monday 00:00) containing `now_ms`.
pub fn local_week_start_ms(now_ms: i64) -> i64 {
    week_start_in(&chrono::Local, now_ms)
}

/// Start of the day containing `now_ms` in `tz`.
pub fn day_start_in<Tz: TimeZone>(tz: &Tz, now_ms: i64) -> i64 {
    match tz.timestamp_millis_opt(now_ms).earliest() {
        Some(now) => date_start_in(tz, now.date_naive(), now_ms),
        None => now_ms,
    }
}

/// Start of the Monday on or before the day containing `now_ms` in `tz`.
pub fn week_start_in<Tz: TimeZone>(tz: &Tz, now_ms: i64) -> i64 {
    let Some(now) = tz.timestamp_millis_opt(now_ms).earliest() else {
        return now_ms;
    };
    let today = now.date_naive();
    let back = u64::from(today.weekday().num_days_from_monday());
    match today.checked_sub_days(Days::new(back)) {
        Some(monday) => date_start_in(tz, monday, now_ms),
        None => now_ms,
    }
}

/// First instant of `date` in `tz`. Where DST skips midnight (e.g. some zones jump
/// 00:00 → 01:00), the day starts at its first existing instant.
fn date_start_in<Tz: TimeZone>(tz: &Tz, date: NaiveDate, fallback_ms: i64) -> i64 {
    match date.and_time(NaiveTime::MIN).and_local_timezone(tz.clone()) {
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => t.timestamp_millis(),
        LocalResult::None => {
            // Midnight does not exist that day: find the first local minute that does.
            (1..=180)
                .filter_map(|m| {
                    let t = date.and_time(NaiveTime::MIN) + chrono::Duration::minutes(m);
                    t.and_local_timezone(tz.clone()).earliest()
                })
                .map(|t| t.timestamp_millis())
                .next()
                .unwrap_or(fallback_ms)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    // 2026-10-06T10:30:00Z
    const NOW: i64 = 1_791_282_600_000;

    #[test]
    fn day_starts_at_utc_midnight_in_utc() {
        assert_eq!(day_start_in(&chrono::Utc, NOW), 1_791_244_800_000);
    }

    #[test]
    fn day_starts_at_local_midnight_east_of_utc() {
        // UTC+3: 13:30 local → local midnight is 21:00Z the previous day.
        let tz = FixedOffset::east_opt(3 * 3600).expect("tz");
        assert_eq!(day_start_in(&tz, NOW), 1_791_244_800_000 - 3 * 3_600_000);
    }

    #[test]
    fn day_start_west_of_utc_can_be_the_previous_utc_date() {
        // UTC-11: 23:30 local on Oct 5 → local midnight is Oct 5 11:00Z.
        let tz = FixedOffset::west_opt(11 * 3600).expect("tz");
        assert_eq!(
            day_start_in(&tz, NOW),
            1_791_244_800_000 - 86_400_000 + 11 * 3_600_000
        );
    }

    // Monday / Sunday / next Monday of the same week, 10:30Z, and their UTC midnights.
    const MON: (i64, i64) = (1_791_196_200_000, 1_791_158_400_000);
    const SUN: i64 = 1_791_714_600_000;
    const NEXT_MON: (i64, i64) = (1_791_801_000_000, 1_791_763_200_000);

    #[test]
    fn week_starts_at_monday_midnight() {
        // NOW is a Tuesday.
        assert_eq!(week_start_in(&chrono::Utc, NOW), MON.1);
        assert_eq!(
            week_start_in(&chrono::Utc, SUN),
            MON.1,
            "Sunday still belongs to it"
        );
    }

    #[test]
    fn on_monday_the_week_starts_that_same_midnight() {
        assert_eq!(week_start_in(&chrono::Utc, MON.0), MON.1);
        assert_eq!(
            week_start_in(&chrono::Utc, MON.1),
            MON.1,
            "exactly at midnight"
        );
        assert_eq!(week_start_in(&chrono::Utc, NEXT_MON.0), NEXT_MON.1);
        assert_eq!(
            week_start_in(&chrono::Utc, MON.1 - 1),
            MON.1 - 7 * 86_400_000
        );
    }

    #[test]
    fn week_start_follows_the_local_calendar() {
        // UTC+3: Monday 01:00 local is still Sunday 22:00Z — already the new local week.
        let tz = FixedOffset::east_opt(3 * 3600).expect("tz");
        let monday_0100_local = MON.1 - 2 * 3_600_000;
        assert_eq!(week_start_in(&tz, monday_0100_local), MON.1 - 3 * 3_600_000);
    }

    #[test]
    fn local_week_start_is_at_most_a_week_before_now() {
        let w = local_week_start_ms(NOW);
        assert!(w <= local_midnight_ms(NOW) && NOW - w < 8 * 86_400_000);
    }

    #[test]
    fn local_midnight_is_at_most_a_day_before_now() {
        let m = local_midnight_ms(NOW);
        assert!(m <= NOW && NOW - m < 25 * 3_600_000);
    }
}
