use super::*;
use chrono::TimeZone;

const FILE: &str = "Study/midterm-1/plan.md";

fn plan(facts: &str, sessions: &str) -> String {
    format!(
        "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n{facts}\n## 14. Sessions\n\n{sessions}\n"
    )
}

fn sessions_of(target: &str, text: &str) -> PlanSessions {
    PlanSessions {
        target: target.to_owned(),
        title: fact(text, "Target"),
        date: fact(text, "When")
            .as_deref()
            .and_then(super::super::first_date),
        modified: Some(Utc.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).unwrap()),
        lines: sessions::lines(FILE, text),
    }
}

/// A Tuesday.
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 17).unwrap()
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 17, 6, 30, 0).unwrap()
}

fn file_for(plans: &[PlanSessions]) -> (String, Vec<String>) {
    let (events, left_out) = events(plans, Vec::new(), today());
    (ics("folder-1", &events, now()), left_out.notes())
}

/// The file's content lines, unfolded, after checking every physical line.
fn unfolded(file: &str) -> Vec<String> {
    assert!(file.ends_with("\r\n"), "the file ends with CRLF");
    let body = file.strip_suffix("\r\n").unwrap();
    assert!(
        !body.contains('\n') || body.split("\r\n").all(|line| !line.contains('\n')),
        "every line ends with CRLF, never a bare LF"
    );
    let mut lines: Vec<String> = Vec::new();
    for physical in body.split("\r\n") {
        assert!(
            physical.len() <= 75,
            "{physical:?} is {} octets",
            physical.len()
        );
        match physical.strip_prefix(' ') {
            Some(rest) => lines
                .last_mut()
                .expect("a continuation follows a line")
                .push_str(rest),
            None => lines.push(physical.to_owned()),
        }
    }
    lines
}

fn property<'a>(lines: &'a [String], name: &str) -> Vec<&'a str> {
    lines
        .iter()
        .filter_map(|line| line.strip_prefix(&format!("{name}:")))
        .collect()
}

#[test]
fn each_session_line_is_one_weekly_event_on_its_days_at_its_start() {
    let text = plan(
        "| Target | Midterm 1 |\n| When | 2026-12-04 |\n",
        "- If it is Monday to Friday at 08:00, then I do the due cold redos at my desk before class.\n\
         - If it is Tuesday and Thursday at 18:30 to 19:45, then I sit the next block at the library.\n\
         - If I miss the morning, then I do them after dinner.\n",
    );
    let (file, notes) = file_for(&[sessions_of("midterm-1", &text)]);
    assert_eq!(notes, Vec::<String>::new());
    let lines = unfolded(&file);
    assert_eq!(lines.first().map(String::as_str), Some("BEGIN:VCALENDAR"));
    assert_eq!(lines.last().map(String::as_str), Some("END:VCALENDAR"));
    assert_eq!(property(&lines, "VERSION"), ["2.0"]);
    assert_eq!(property(&lines, "PRODID").len(), 1);
    assert_eq!(
        property(&lines, "BEGIN")
            .iter()
            .filter(|v| **v == "VEVENT")
            .count(),
        2,
        "the backup is no event"
    );
    assert_eq!(
        property(&lines, "SUMMARY"),
        [
            "Do the due cold redos at my desk before class",
            "Sit the next block at the library"
        ]
    );
    // Tuesday the 17th is the first weekday on or after today; the second line's first day is today too.
    assert_eq!(
        property(&lines, "DTSTART"),
        ["20261117T080000", "20261117T183000"]
    );
    assert_eq!(
        property(&lines, "DTEND"),
        ["20261117T083000", "20261117T194500"]
    );
    assert_eq!(
        property(&lines, "RRULE"),
        [
            "FREQ=WEEKLY;WKST=MO;BYDAY=MO,TU,WE,TH,FR;UNTIL=20261204T235959",
            "FREQ=WEEKLY;WKST=MO;BYDAY=TU,TH;UNTIL=20261204T235959"
        ]
    );
    assert_eq!(
        property(&lines, "DTSTAMP"),
        ["20261117T063000Z", "20261117T063000Z"]
    );
    assert!(
        property(&lines, "DESCRIPTION")[0]
            .starts_with("If it is Monday to Friday at 08:00\\, then I do"),
        "{:?}",
        property(&lines, "DESCRIPTION")
    );
    assert!(
        property(&lines, "DESCRIPTION")[0].ends_with("\\nFrom your Magnesium plan for Midterm 1.")
    );
}

#[test]
fn a_first_day_is_the_first_of_its_days_on_or_after_today() {
    let text = plan(
        "",
        "- If it is Saturday and Monday at 09:00, then I read the gap log.\n",
    );
    let (file, _) = file_for(&[sessions_of("reading", &text)]);
    let lines = unfolded(&file);
    assert_eq!(property(&lines, "DTSTART"), ["20261121T090000"]);
    assert_eq!(
        property(&lines, "RRULE"),
        ["FREQ=WEEKLY;WKST=MO;BYDAY=MO,SA"],
        "a plan with no date has no end"
    );
}

#[test]
fn an_end_before_the_start_is_the_next_morning() {
    let text = plan(
        "",
        "- If it is Tuesday at 23:30 to 00:15, then I do the quick checks.\n",
    );
    let (file, _) = file_for(&[sessions_of("late", &text)]);
    let lines = unfolded(&file);
    assert_eq!(property(&lines, "DTSTART"), ["20261117T233000"]);
    assert_eq!(property(&lines, "DTEND"), ["20261118T001500"]);
}

#[test]
fn backups_none_planned_and_the_template_make_no_event_and_no_note() {
    for lines in [
        "none planned\n",
        "- If I miss Monday, then I do them on Tuesday.\n",
        "- If it is <days> at <HH:MM>, then I <what I do> <where>.\n",
        "",
    ] {
        let text = plan("", lines);
        let plans = [sessions_of("midterm-1", &text)];
        let (found, left_out) = events(&plans, Vec::new(), today());
        assert!(found.is_empty(), "{lines:?}");
        assert!(left_out.notes().is_empty(), "{lines:?}");
    }
}

#[test]
fn a_line_it_cannot_read_is_a_note_with_its_line_and_never_an_event() {
    let text = plan(
        "",
        "- If it is Monday at 8am, then I do the due cold redos.\n\
         - If it is Wednesday at 07:00, then I do the due cold redos.\n",
    );
    let plans = [sessions_of("midterm-1", &text)];
    let (found, left_out) = events(&plans, Vec::new(), today());
    let notes = left_out.notes();
    assert_eq!(found.len(), 1);
    let line = text.lines().position(|l| l.contains("8am")).unwrap() + 1;
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].starts_with(&format!(
            "{FILE}, line {line}: not read, because the start time"
        )),
        "{notes:?}"
    );
}

#[test]
fn a_plan_whose_date_has_passed_is_left_out_with_a_note() {
    let text = plan(
        "| When | 2026-11-16 |\n",
        "- If it is Monday at 08:00, then I do the due cold redos.\n",
    );
    let plans = [sessions_of("midterm-1", &text)];
    let (found, left_out) = events(&plans, Vec::new(), today());
    assert!(found.is_empty());
    assert_eq!(
        left_out.notes(),
        ["Study/midterm-1/plan.md: left out, because its date, 2026-11-16, has passed"]
    );
    let on_the_day = plan(
        "| When | 2026-11-17 |\n",
        "- If it is Tuesday at 08:00, then I do the due cold redos.\n",
    );
    let plans = [sessions_of("midterm-1", &on_the_day)];
    assert_eq!(
        events(&plans, Vec::new(), today()).0.len(),
        1,
        "the test's own day still counts"
    );
}

fn uids_of(plans: &[PlanSessions], folder: &str, day: NaiveDate) -> Vec<String> {
    let (events, _) = events(plans, Vec::new(), day);
    property(&unfolded(&ics(folder, &events, now())), "UID")
        .iter()
        .map(|s| s.to_string())
        .collect()
}

#[test]
fn the_same_session_keeps_its_uid_from_one_file_to_the_next() {
    let text = plan(
        "",
        "- If it is Monday at 08:00, then I do the due cold redos.\n- If it is Friday at 17:00, then I sit a paper.\n",
    );
    let plans = [sessions_of("midterm-1", &text)];
    let first = uids_of(&plans, "folder-1", today());
    assert_eq!(
        first,
        uids_of(&plans, "folder-1", today() + Duration::days(9))
    );
    assert_eq!(first.len(), 2);
    assert_ne!(first[0], first[1]);
    // Reworded, the same session keeps its UID, so its event is renamed.
    let reworded = plan(
        "",
        "- If it is Monday at 08:00, then I redo the gap log at the library.\n- If it is Friday at 17:00, then I sit a paper.\n",
    );
    assert_eq!(
        uids_of(&[sessions_of("midterm-1", &reworded)], "folder-1", today()),
        first
    );
    // Another plan, or another study folder, never shares one.
    assert!(
        uids_of(&[sessions_of("final", &text)], "folder-1", today())
            .iter()
            .all(|u| !first.contains(u))
    );
    assert!(
        uids_of(&plans, "folder-2", today())
            .iter()
            .all(|u| !first.contains(u))
    );
    let (events_now, _) = events(&plans, Vec::new(), today());
    assert!(
        !ics("folder-1", &events_now, now()).contains("folder-1"),
        "the folder's id is not in the file"
    );
}

#[test]
fn a_line_taken_out_or_put_in_above_a_session_leaves_its_uid_alone() {
    let both = plan(
        "",
        "- If it is Monday at 08:00, then I do the due cold redos.\n- If it is Friday at 17:00, then I sit a paper.\n",
    );
    let before = uids_of(&[sessions_of("midterm-1", &both)], "folder-1", today());
    // Monday's line taken out: Friday's event is the same event, and no event takes Monday's UID.
    let friday = plan("", "- If it is Friday at 17:00, then I sit a paper.\n");
    assert_eq!(
        uids_of(&[sessions_of("midterm-1", &friday)], "folder-1", today()),
        [before[1].clone()]
    );
    // A line put in first: both earlier sessions keep theirs, and the new one has its own.
    let three = plan(
        "",
        "- If it is Sunday at 19:00, then I read the gap log.\n- If it is Monday at 08:00, then I do the due cold redos.\n- If it is Friday at 17:00, then I sit a paper.\n",
    );
    let after = uids_of(&[sessions_of("midterm-1", &three)], "folder-1", today());
    assert_eq!(&after[1..], &before[..]);
    assert!(!before.contains(&after[0]));
    // A session moved to another time is another event; the old one is not changed into it.
    let moved = plan(
        "",
        "- If it is Monday at 07:30, then I do the due cold redos.\n- If it is Friday at 17:00, then I sit a paper.\n",
    );
    let moved_uids = uids_of(&[sessions_of("midterm-1", &moved)], "folder-1", today());
    assert_ne!(moved_uids[0], before[0]);
    assert_eq!(moved_uids[1], before[1]);
    // Two lines on the same days at the same times are two events.
    let twice = plan(
        "",
        "- If it is Monday at 08:00, then I do the due cold redos.\n- If it is Monday at 08:00, then I bring the gap log.\n",
    );
    let twins = uids_of(&[sessions_of("midterm-1", &twice)], "folder-1", today());
    assert_eq!(twins.len(), 2);
    assert_ne!(twins[0], twins[1]);
    assert_eq!(twins[0], before[0]);
}

#[test]
fn a_session_whose_first_day_comes_after_the_plan_s_date_is_left_out() {
    // Today is Tuesday the 17th and the test is Wednesday the 18th: Saturday's session would come after it.
    let text = plan(
        "| When | 2026-11-18 |\n",
        "- If it is Saturday at 10:00, then I sit the next block.\n- If it is Wednesday at 07:00, then I do the due cold redos.\n",
    );
    let plans = [sessions_of("midterm-1", &text)];
    let (found, left_out) = events(&plans, Vec::new(), today());
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].first,
        NaiveDate::from_ymd_opt(2026, 11, 18).unwrap()
    );
    assert_eq!(
        left_out.notes(),
        [
            "Study/midterm-1/plan.md: \"Sit the next block\" left out, because its first day, 2026-11-21, comes after the plan's date, 2026-11-18"
        ]
    );
    let lines = unfolded(&ics("folder-1", &found, now()));
    let start = property(&lines, "DTSTART")[0].to_owned();
    let rule = property(&lines, "RRULE")[0].to_owned();
    let until = rule.split("UNTIL=").nth(1).unwrap();
    assert!(
        start.as_str() <= until,
        "DTSTART {start} is not after UNTIL {until}"
    );
}

#[test]
fn a_plan_whose_date_has_passed_says_nothing_of_its_unreadable_lines() {
    let old = plan(
        "| When | 2026-10-01 |\n",
        "- If it is Monday at 8am, then I do the due cold redos.\n",
    );
    let plans = [sessions_of("midterm-1", &old)];
    let (found, left_out) = events(&plans, Vec::new(), today());
    assert!(found.is_empty());
    assert!(
        left_out.unreadable.is_empty(),
        "a line in an ended plan cannot matter"
    );
    let value = listed(&found, &left_out);
    assert_eq!(value["unreadable"], json!([]));
}

#[test]
fn a_plan_file_that_cannot_be_read_is_said_in_the_json_too() {
    let (found, left_out) = events(&[], vec!["Study/huge/plan.md".to_owned()], today());
    assert!(found.is_empty());
    assert_eq!(
        listed(&found, &left_out)["unreadable_plans"],
        json!(["Study/huge/plan.md"])
    );
    assert_eq!(
        left_out.notes(),
        ["Study/huge/plan.md: not read, because it is too large, not text, or not a regular file"]
    );
}

#[test]
fn each_file_is_a_later_revision_and_says_when_the_plan_changed() {
    let text = plan(
        "",
        "- If it is Monday at 08:00, then I do the due cold redos.\n",
    );
    let plans = [sessions_of("midterm-1", &text)];
    let (found, _) = events(&plans, Vec::new(), today());
    let first = unfolded(&ics("folder-1", &found, now()));
    let later = unfolded(&ics("folder-1", &found, now() + Duration::minutes(5)));
    let seq = |lines: &[String]| property(lines, "SEQUENCE")[0].parse::<i64>().unwrap();
    assert!(seq(&later) > seq(&first));
    assert!(
        seq(&first) < i64::from(i32::MAX),
        "SEQUENCE is an INTEGER (RFC 5545, 3.3.8)"
    );
    assert_eq!(property(&first, "LAST-MODIFIED"), ["20261116T090000Z"]);
}

#[test]
fn text_is_escaped_and_long_lines_fold_without_splitting_a_character() {
    let text = plan(
        "| Target | Chemie; Ü-Kurs, Teil 2 |\n",
        "- If it is Monday at 08:00, then I do the Übungsblätter, Aufgaben 1–4; then the gap log \\ notes, at the Universitätsbibliothek café 📚📚📚 near the Hauptgebäude, 自习室.\n",
    );
    let (file, _) = file_for(&[sessions_of("chemie", &text)]);
    assert!(
        file.split("\r\n").any(|line| line.starts_with(' ')),
        "the long lines fold"
    );
    let lines = unfolded(&file);
    let summary = property(&lines, "SUMMARY")[0];
    assert!(
        summary.starts_with("Do the Übungsblätter\\, Aufgaben 1–4\\; then the gap log \\\\ notes"),
        "{summary}"
    );
    assert!(
        property(&lines, "DESCRIPTION")[0]
            .ends_with("From your Magnesium plan for Chemie\\; Ü-Kurs\\, Teil 2.")
    );
}

#[test]
fn an_action_is_the_words_after_then_i() {
    for (line, expected) in [
        (
            "If it is Monday at 08:00, then I do the due cold redos at my desk.",
            "Do the due cold redos at my desk",
        ),
        (
            "If it's 06:45, then i run through the quick checks",
            "Run through the quick checks",
        ),
        (
            "If it is Monday at 08:00, Then I sit Paper 2.",
            "Sit Paper 2",
        ),
        ("If it is Monday at 08:00, then we meet.", "We meet"),
        ("If it is Monday at 08:00, then", "Study session"),
    ] {
        assert_eq!(action(line), expected, "{line}");
    }
}
