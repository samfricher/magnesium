use std::collections::BTreeSet;

use chrono::{Days, NaiveDate};
use serde::Serialize;

pub const SHOWN_DAYS: u64 = 35;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Activity {
    pub today: String,
    pub days: Vec<String>,
    pub run: u32,
    pub studied_today: bool,
}

pub fn key(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

pub fn parse(key: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(key, "%Y-%m-%d").ok()
}

const REST_EVERY_DAYS: i64 = 7;

fn run_at(studied: &[NaiveDate], today: NaiveDate) -> u32 {
    let has = |day: NaiveDate| studied.binary_search(&day).is_ok();
    let before = |day: NaiveDate| day.checked_sub_days(Days::new(1));
    let Some(mut day) = (if has(today) {
        Some(today)
    } else {
        before(today)
    }) else {
        return 0;
    };
    let mut run = 0;
    let mut last_rest: Option<NaiveDate> = None;
    loop {
        if has(day) {
            run += 1;
        } else {
            let rested_this_week =
                last_rest.is_some_and(|rest| (rest - day).num_days() < REST_EVERY_DAYS);
            if rested_this_week || !before(day).is_some_and(has) {
                break;
            }
            last_rest = Some(day);
        }
        match before(day) {
            Some(earlier) => day = earlier,
            None => break,
        }
    }
    run
}

pub fn summarize(days: &BTreeSet<NaiveDate>, today: NaiveDate) -> Activity {
    let studied: Vec<NaiveDate> = days.iter().copied().filter(|day| *day <= today).collect();
    let first_shown = today
        .checked_sub_days(Days::new(SHOWN_DAYS - 1))
        .unwrap_or(today);
    Activity {
        today: key(today),
        days: studied
            .iter()
            .filter(|day| **day >= first_shown)
            .map(|day| key(*day))
            .collect(),
        run: run_at(&studied, today),
        studied_today: studied.binary_search(&today).is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        parse(text).unwrap()
    }

    fn counts(days: &[&str]) -> BTreeSet<NaiveDate> {
        days.iter().map(|d| day(d)).collect()
    }

    #[test]
    fn a_run_counts_days_in_a_row_up_to_today() {
        let studied = counts(&["2026-11-07", "2026-11-08", "2026-11-09", "2026-11-10"]);
        let activity = summarize(&studied, day("2026-11-10"));
        assert_eq!(activity.run, 4);
        assert!(activity.studied_today);
    }

    #[test]
    fn today_does_not_end_a_run_before_it_is_over() {
        let studied = counts(&["2026-11-08", "2026-11-09"]);
        let morning = summarize(&studied, day("2026-11-10"));
        assert_eq!(morning.run, 2);
        assert!(!morning.studied_today);
    }

    #[test]
    fn one_missed_day_in_a_week_is_a_rest_day() {
        let studied = counts(&["2026-11-02", "2026-11-03", "2026-11-05", "2026-11-06"]);
        let friday = summarize(&studied, day("2026-11-06"));
        assert_eq!(
            friday.run, 4,
            "the rest day is not counted and does not end the run"
        );
        let two_days_on = summarize(&counts(&["2026-11-02", "2026-11-03"]), day("2026-11-05"));
        assert_eq!(two_days_on.run, 2);
    }

    #[test]
    fn two_missed_days_in_a_row_end_a_run() {
        let studied = counts(&["2026-11-02", "2026-11-03"]);
        assert_eq!(summarize(&studied, day("2026-11-06")).run, 0);
        let again = counts(&["2026-11-02", "2026-11-03", "2026-11-06"]);
        assert_eq!(summarize(&again, day("2026-11-06")).run, 1);
    }

    #[test]
    fn a_second_missed_day_within_a_week_ends_a_run() {
        let studied = counts(&[
            "2026-11-01",
            "2026-11-02",
            "2026-11-04",
            "2026-11-05",
            "2026-11-07",
            "2026-11-08",
        ]);
        assert_eq!(summarize(&studied, day("2026-11-08")).run, 4);
        let spaced = counts(&[
            "2026-11-01",
            "2026-11-03",
            "2026-11-04",
            "2026-11-05",
            "2026-11-06",
            "2026-11-07",
            "2026-11-08",
            "2026-11-10",
        ]);
        assert_eq!(summarize(&spaced, day("2026-11-10")).run, 8);
    }

    #[test]
    fn only_the_last_five_weeks_are_listed_and_a_day_after_today_is_not_counted() {
        let studied = counts(&["2026-09-01", "2026-10-07", "2026-11-10", "2026-11-12"]);
        let activity = summarize(&studied, day("2026-11-10"));
        assert_eq!(activity.days, ["2026-10-07", "2026-11-10"]);
        assert_eq!(activity.run, 1);
    }

    #[test]
    fn nothing_studied_gives_no_run() {
        let activity = summarize(&BTreeSet::new(), day("2026-11-10"));
        assert_eq!(activity.run, 0);
        assert!(activity.days.is_empty());
        assert_eq!(activity.today, "2026-11-10");
    }
}
