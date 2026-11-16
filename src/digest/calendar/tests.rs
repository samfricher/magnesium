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
    let (events, left_out) = events(plans, today());
    (ics("folder-1", &events, today(), now()), left_out.notes())
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
    assert!(property(&lines, "DESCRIPTION")[0].ends_with("\\nFrom your plan for Midterm 1."));
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
        let (found, left_out) = events(&plans, today());
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
    let (found, left_out) = events(&plans, today());
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
    let (found, left_out) = events(&plans, today());
    assert!(found.is_empty());
    assert_eq!(
        left_out.notes(),
        ["Study/midterm-1/plan.md: left out, because its date, 2026-11-16, has passed"]
    );
    let on_the_day = plan(
        "| When | 2026-11-17 |\n",
        "- If it is Monday at 08:00, then I do the due cold redos.\n",
    );
    let plans = [sessions_of("midterm-1", &on_the_day)];
    assert_eq!(
        events(&plans, today()).0.len(),
        1,
        "the test's own day still counts"
    );
}

#[test]
fn the_same_session_keeps_its_uid_from_one_file_to_the_next() {
    let text = plan(
        "",
        "- If it is Monday at 08:00, then I do the due cold redos.\n- If it is Friday at 17:00, then I sit a paper.\n",
    );
    let plans = [sessions_of("midterm-1", &text)];
    let (events_now, _) = events(&plans, today());
    let first = ics("folder-1", &events_now, today(), now());
    let later_day = today() + Duration::days(9);
    let (events_later, _) = events(&plans, later_day);
    let second = ics(
        "folder-1",
        &events_later,
        later_day,
        now() + Duration::days(9),
    );
    let uids = |file: &str| {
        property(&unfolded(file), "UID")
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(uids(&first), uids(&second));
    assert_eq!(uids(&first).len(), 2);
    assert_ne!(uids(&first)[0], uids(&first)[1]);
    // The times changed, the session did not: the same UID updates it.
    let moved = plan(
        "",
        "- If it is Monday at 07:30, then I do the due cold redos.\n- If it is Friday at 17:00, then I sit a paper.\n",
    );
    let moved_plans = [sessions_of("midterm-1", &moved)];
    let (moved_events, _) = events(&moved_plans, today());
    assert_eq!(
        uids(&ics("folder-1", &moved_events, today(), now())),
        uids(&first)
    );
    // Another plan, or another study folder, never shares one.
    let other = [sessions_of("final", &text)];
    let (other_events, _) = events(&other, today());
    assert!(
        uids(&ics("folder-1", &other_events, today(), now()))
            .iter()
            .all(|u| !uids(&first).contains(u))
    );
    assert!(
        uids(&ics("folder-2", &events_now, today(), now()))
            .iter()
            .all(|u| !uids(&first).contains(u))
    );
    assert!(
        !first.contains("folder-1"),
        "the folder's id is not in the file"
    );
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
            .ends_with("From your plan for Chemie\\; Ü-Kurs\\, Teil 2.")
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
