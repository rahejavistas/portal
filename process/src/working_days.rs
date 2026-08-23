use chrono::{Datelike, Duration, NaiveDate, Weekday};
use std::collections::HashSet;

fn is_working_day(date: NaiveDate, holidays: &HashSet<NaiveDate>) -> bool {
    !matches!(date.weekday(), Weekday::Sat | Weekday::Sun) && !holidays.contains(&date)
}

/// `start` plus `working_days` working days (skipping weekends and holidays).
/// `working_days <= 0` returns the next working day on/after `start`.
pub fn add_working_days(
    start: NaiveDate,
    working_days: i32,
    holidays: &HashSet<NaiveDate>,
) -> NaiveDate {
    let mut date = start;
    let mut remaining = working_days.max(0);
    while !is_working_day(date, holidays) {
        date += Duration::days(1);
    }
    while remaining > 0 {
        date += Duration::days(1);
        if is_working_day(date, holidays) {
            remaining -= 1;
        }
    }
    date
}

/// Working days strictly between `from` and `to` (exclusive of `from`, inclusive of
/// `to`), used to convert a pause interval into an SLA-clock offset. Zero if `to <= from`.
pub fn count_working_days_between(
    from: NaiveDate,
    to: NaiveDate,
    holidays: &HashSet<NaiveDate>,
) -> i32 {
    let mut count = 0;
    let mut date = from;
    while date < to {
        date += Duration::days(1);
        if is_working_day(date, holidays) {
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn add_working_days_skips_weekends() {
        let start = date(2026, 8, 21); // Friday
        let holidays = HashSet::new();
        assert_eq!(add_working_days(start, 1, &holidays), date(2026, 8, 24)); // Monday
    }

    #[test]
    fn add_working_days_skips_holidays() {
        let start = date(2026, 8, 24); // Monday
        let mut holidays = HashSet::new();
        holidays.insert(date(2026, 8, 25)); // Tuesday holiday
        assert_eq!(add_working_days(start, 1, &holidays), date(2026, 8, 26));
    }

    #[test]
    fn add_working_days_zero_returns_next_working_day_from_a_weekend_start() {
        let start = date(2026, 8, 22); // Saturday
        let holidays = HashSet::new();
        assert_eq!(add_working_days(start, 0, &holidays), date(2026, 8, 24));
    }

    #[test]
    fn count_working_days_between_skips_weekend() {
        let holidays = HashSet::new();
        // Friday to the following Monday: only Monday counts.
        assert_eq!(
            count_working_days_between(date(2026, 8, 21), date(2026, 8, 24), &holidays),
            1
        );
    }

    #[test]
    fn count_working_days_between_same_day_is_zero() {
        let holidays = HashSet::new();
        assert_eq!(
            count_working_days_between(date(2026, 8, 24), date(2026, 8, 24), &holidays),
            0
        );
    }
}
