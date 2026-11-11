//! The days the learner studied in the window, and the run they make.
//!
//! A day counts when the learner sent the tutor at least one message that
//! day. It is the window's own record: work done in a terminal is not seen
//! here. Nothing in it is learning state, and no proof depends on it.
//!
//! The run is forgiving on purpose. A run that one missed day can end turns
//! a bad day into a reason to stop altogether. So one missed day in a week is
//! a rest day, and the run goes on through it.

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate};
use serde::Serialize;

/// How many days the page draws: five weeks.
pub const SHOWN_DAYS: u64 = 35;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Day {
    /// The day as "2026-11-10".
    pub date: String,
    /// Messages the learner sent that day.
    pub turns: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Activity {
    /// Today as "2026-11-10", so the page and the core agree on the day.
    pub today: String,
    /// The studied days among the last 35, oldest first.
    pub days: Vec<Day>,
    /// Days studied in the run that reaches today. Today does not end a run
    /// before it is over, and one missed day in a week is a rest day that
    /// does not end it either. Rest days are not counted.
    pub run: u32,
    /// Whether the learner has studied today.
    pub studied_today: bool,
}

pub fn key(day: NaiveDate) -> String {
    day.format("%Y-%m-%d").to_string()
}

fn parse(key: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(key, "%Y-%m-%d").ok()
}

/// How many days must pass before another missed day can be a rest day.
const REST_EVERY_DAYS: i64 = 7;

/// The studied days in the run that reaches `today`.
fn run_at(studied: &[NaiveDate], today: NaiveDate) -> u32 {
    let has = |day: NaiveDate| studied.binary_search(&day).is_ok();
    let before = |day: NaiveDate| day.checked_sub_days(Days::new(1));
    // Today is not over, so a day not studied yet is not a missed day.
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
            // A missed day. It is a rest day when the day before it was
            // studied and no other rest day is within a week of it.
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

/// What the page shows, from the count of messages kept for each day.
pub fn summarize(counts: &BTreeMap<String, u32>, today: NaiveDate) -> Activity {
    let studied: Vec<NaiveDate> = counts
        .iter()
        .filter(|(_, turns)| **turns > 0)
        .filter_map(|(day, _)| parse(day))
        .filter(|day| *day <= today)
        .collect();
    let first_shown = today
        .checked_sub_days(Days::new(SHOWN_DAYS - 1))
        .unwrap_or(today);
    Activity {
        today: key(today),
        days: studied
            .iter()
            .filter(|day| **day >= first_shown)
            .map(|day| Day {
                date: key(*day),
                turns: counts.get(&key(*day)).copied().unwrap_or(0),
            })
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

    fn counts(days: &[&str]) -> BTreeMap<String, u32> {
        days.iter().map(|d| ((*d).to_owned(), 2)).collect()
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
        // Monday and Tuesday, Wednesday missed, Thursday and Friday.
        let studied = counts(&["2026-11-02", "2026-11-03", "2026-11-05", "2026-11-06"]);
        let friday = summarize(&studied, day("2026-11-06"));
        assert_eq!(
            friday.run, 4,
            "the rest day is not counted and does not end the run"
        );
        // Yesterday missed and today not studied yet: yesterday is the rest day.
        let two_days_on = summarize(&counts(&["2026-11-02", "2026-11-03"]), day("2026-11-05"));
        assert_eq!(two_days_on.run, 2);
    }

    #[test]
    fn two_missed_days_in_a_row_end_a_run() {
        let studied = counts(&["2026-11-02", "2026-11-03"]);
        // Wednesday and Thursday both missed.
        assert_eq!(summarize(&studied, day("2026-11-06")).run, 0);
        // Studied again on Friday: a new run of one.
        let again = counts(&["2026-11-02", "2026-11-03", "2026-11-06"]);
        assert_eq!(summarize(&again, day("2026-11-06")).run, 1);
    }

    #[test]
    fn a_second_missed_day_within_a_week_ends_a_run() {
        // Studied 1, 2, 4, 5, 7, 8 November: the 3rd and the 6th are missed.
        let studied = counts(&[
            "2026-11-01",
            "2026-11-02",
            "2026-11-04",
            "2026-11-05",
            "2026-11-07",
            "2026-11-08",
        ]);
        // The 6th is the rest day. The 3rd is within a week of it, so the run starts on the 4th.
        assert_eq!(summarize(&studied, day("2026-11-08")).run, 4);
        // Missed days a week or more apart are each a rest day.
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
    fn only_the_last_five_weeks_are_listed_and_days_with_nothing_are_not() {
        let mut studied = counts(&["2026-09-01", "2026-10-07", "2026-11-10"]);
        studied.insert("2026-11-09".into(), 0);
        studied.insert("not a day".into(), 4);
        // A clock set back must not put a later day into the run.
        studied.insert("2026-11-12".into(), 1);
        let activity = summarize(&studied, day("2026-11-10"));
        let listed: Vec<&str> = activity.days.iter().map(|d| d.date.as_str()).collect();
        assert_eq!(listed, ["2026-10-07", "2026-11-10"]);
        assert_eq!(activity.days[1].turns, 2);
        assert_eq!(activity.run, 1);
    }

    #[test]
    fn nothing_studied_gives_no_run() {
        let activity = summarize(&BTreeMap::new(), day("2026-11-10"));
        assert_eq!(activity.run, 0);
        assert!(activity.days.is_empty());
        assert_eq!(activity.today, "2026-11-10");
    }
}
