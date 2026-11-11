use super::*;
use serde_json::{Value, json};

const FILE: &str = "Study/midterm-1/plan.md";

const PLAN: &str = "# Midterm 1: study plan\n\n## 6. Blocks\n\n| # | Block | Purpose |\n|---|---|---|\n| 1 | Diagnostic | find gaps |\n\n## 7. Must know cold\n\n- Km\n";

fn sessions_on(lines: &str, day: Weekday) -> Value {
    let plan = format!("{PLAN}\n## 14. Sessions\n\n{lines}\n");
    let today = NaiveDate::from_isoywd_opt(2026, 43, day).unwrap();
    json!(read(FILE, &plan, today).today)
}

const WEEK: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

#[test]
fn planned_sessions_today_gives_the_start_times_of_the_lines_that_fall_today() {
    let lines = "- If it is Monday to Friday at 08:00, then I do the due cold redos at my desk before class.\n\
            - If it is Tuesday and Thursday at 18:30, then I sit the next block at the library.\n\
            - If it is Tuesday at 08:00, then I bring the gap log.\n\
            - If I miss the morning, then I do them after dinner.\n";
    assert_eq!(sessions_on(lines, Weekday::Tue), json!(["08:00", "18:30"]));
    assert_eq!(sessions_on(lines, Weekday::Wed), json!(["08:00"]));
    assert_eq!(sessions_on(lines, Weekday::Sat), json!([]));
}

#[test]
fn a_sessions_line_falls_on_each_day_its_days_name() {
    for (days, falls) in [
        ("Monday to Friday", "MTWTF.."),
        ("Mon-Fri", "MTWTF.."),
        ("Mon–Fri", "MTWTF.."),
        ("Mon—Fri", "MTWTF.."),
        ("Friday to Monday", "M...FSS"),
        ("Monday, Wednesday and Friday", "M.W.F.."),
        ("Monday, Wednesday, and Friday", "M.W.F.."),
        ("Tuesday or Thursday", ".T.T..."),
        ("Tue and Thurs", ".T.T..."),
        ("Saturdays", ".....S."),
        ("every Sunday", "......S"),
        ("weekdays", "MTWTF.."),
        ("a weekday", "MTWTF.."),
        ("the weekend", ".....SS"),
        ("Saturday or Sunday", ".....SS"),
        ("every day", "MTWTFSS"),
        ("any day", "MTWTFSS"),
        ("weekends", ".....SS"),
        ("Monday through Friday", "MTWTF.."),
        ("Mon, Weds and Fri", "M.W.F.."),
        ("each weekday", "MTWTF.."),
        ("everyday", "MTWTFSS"),
    ] {
        let line = format!("- **If it is {days} at 7:15, then** I do the due cold redos.\n");
        for (day, mark) in WEEK.iter().zip(falls.chars()) {
            let expected = if mark == '.' {
                json!([])
            } else {
                json!(["07:15"])
            };
            assert_eq!(sessions_on(&line, *day), expected, "{days} on {day}");
        }
    }
    for day in WEEK {
        for line in [
            "If it's 06:45, then I do the due cold redos.\n",
            "If it’s 06:45, then I do the due cold redos.\n",
        ] {
            assert_eq!(
                sessions_on(line, day),
                json!(["06:45"]),
                "a line without days falls every day: {line}"
            );
        }
    }
}

#[test]
fn a_time_range_gives_its_start_time() {
    let lines = "- If it is Tuesday at 07:00 to 08:00, then I do the due cold redos.\n\
            - If it is Tuesday at 12:30-13:30, then I sit a block at the library.\n\
            - If it is Tuesday at 18:00–19:00, then I read the gap log.\n\
            - If it is Tuesday at 8:05, then I bring the key.\n";
    assert_eq!(
        sessions_on(lines, Weekday::Tue),
        json!(["07:00", "08:05", "12:30", "18:00"])
    );
}

#[test]
fn a_session_line_inside_an_html_comment_is_not_read() {
    let lines = "<!-- Old times:\n\
            - If it is Monday at 06:00, then I do the due cold redos.\n\
            -->\n\
            - <!-- If it is Monday at 05:00, then I run. --> If it is Tuesday at 08:00, then I do the due cold redos.\n";
    assert_eq!(sessions_on(lines, Weekday::Mon), json!([]));
    assert_eq!(sessions_on(lines, Weekday::Tue), json!(["08:00"]));
    assert_eq!(
        sessions_on(
            "<!-- unclosed\n- If it is Tuesday at 06:00, then I do the due cold redos.\n",
            Weekday::Tue
        ),
        json!([]),
        "an unclosed comment hides the rest, as Markdown shows it"
    );
}

#[test]
fn a_sessions_heading_inside_a_comment_or_a_code_fence_is_not_the_section() {
    let plan = format!(
        "{PLAN}\n<!-- Old plan:\n## 7. Sessions\n\n- If it is Tuesday at 06:00, then I do the due cold redos.\n-->\n\n\
             ```markdown\n## 7. Sessions\n\n- If it is Tuesday at 05:00, then I do the due cold redos.\n```\n\n\
             ## 7. Sessions\n\n- If it is Tuesday at 08:00, then I do the due cold redos.\n"
    );
    let tuesday = NaiveDate::from_isoywd_opt(2026, 43, Weekday::Tue).unwrap();
    assert_eq!(json!(read(FILE, &plan, tuesday).today), json!(["08:00"]));
}

#[test]
fn a_session_line_inside_a_code_fence_is_not_read() {
    let lines = "```\n- If it is Tuesday at 06:00, then I do the due cold redos.\n```\n\
            ~~~\n- If it is Tuesday at 07:00, then I do the due cold redos.\n~~~\n\
            - If it is Tuesday at 08:00, then I do the due cold redos.\n";
    assert_eq!(sessions_on(lines, Weekday::Tue), json!(["08:00"]));
}

#[test]
fn a_sessions_line_the_code_cannot_read_gives_no_session() {
    for day in WEEK {
        assert_eq!(sessions_on(UNREADABLE_SESSIONS, day), json!([]), "{day}");
    }
}

#[test]
fn an_if_then_line_outside_sessions_is_not_a_session() {
    let checklist =
        "\n## 11. Assessment-day checklist\n\n- If it is 10:00, then I move on to Q6.\n";
    let plan = format!("{PLAN}{checklist}");
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    assert_eq!(json!(read(FILE, &plan, today).today), json!([]));
    assert_eq!(
        sessions_on(
            &format!("If it is 08:00, then I do the due cold redos.\n{checklist}"),
            Weekday::Tue
        ),
        json!(["08:00"])
    );
}

fn unreadable_in(plan: &str) -> Vec<(usize, String)> {
    let today = NaiveDate::from_isoywd_opt(2026, 43, Weekday::Tue).unwrap();
    read(FILE, plan, today)
        .unreadable
        .into_iter()
        .map(|item| {
            assert_eq!(item.file, FILE);
            assert_eq!(item.section, Some("Sessions"));
            (item.line.unwrap(), item.reason)
        })
        .collect()
}

fn line_of(text: &str, needle: &str) -> usize {
    text.lines().position(|line| line.contains(needle)).unwrap() + 1
}

#[test]
fn every_line_the_code_cannot_read_is_reported_at_its_line() {
    let plan = format!("{PLAN}\n## 14. Sessions\n\n{UNREADABLE_SESSIONS}");
    let first = line_of(&plan, "except Wednesday");
    let backup = line_of(&plan, "If I miss Monday");
    let template = line_of(&plan, "<HH:MM>");
    let expected = (first..first + UNREADABLE_SESSIONS.lines().count())
        .filter(|line| *line != backup && *line != template)
        .collect::<Vec<_>>();
    let reported = unreadable_in(&plan)
        .into_iter()
        .map(|(line, _)| line)
        .collect::<Vec<_>>();
    assert_eq!(reported, expected);
}

#[test]
fn a_backup_none_planned_the_template_lines_and_blanks_are_not_reported() {
    for lines in [
        "- If I miss the morning, then I do them after dinner.\n",
        "none planned\n",
        "**None planned.**\n",
        "- If it is <days> at <HH:MM>, then I <what I do> <where>.\n- If I miss <a session>, then I <what I do instead, and when>.\n",
        "\n\n",
    ] {
        let plan = format!("{PLAN}\n## 14. Sessions\n\n{lines}");
        assert_eq!(unreadable_in(&plan), [], "{lines:?}");
    }
}

#[test]
fn a_readable_session_is_kept_beside_one_that_is_not() {
    let plan = format!(
        "{PLAN}\n## 14. Sessions\n\n- If it is Tuesday at 08:00, then I do the due cold redos.\n- If it is Tuesday at 7pm, then I sit a block.\n"
    );
    let today = NaiveDate::from_isoywd_opt(2026, 43, Weekday::Tue).unwrap();
    let sessions = read(FILE, &plan, today);
    assert_eq!(sessions.today, ["08:00"]);
    assert_eq!(unreadable_in(&plan).len(), 1);
    assert_eq!(unreadable_in(&plan)[0].0, line_of(&plan, "7pm"));
}

#[test]
fn prose_in_the_section_is_reported() {
    let plan = format!("{PLAN}\n## 14. Sessions\n\nMornings suit me best.\n");
    let reported = unreadable_in(&plan);
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].0, line_of(&plan, "Mornings"));
    assert!(reported[0].1.contains("If it is"), "{}", reported[0].1);
}

#[test]
fn each_reason_names_the_part_to_fix() {
    for (line, part) in [
        (
            "If it is Funday at 08:00, then I do the due cold redos.",
            "days",
        ),
        (
            "If it is Monday at 7pm, then I do the due cold redos.",
            "start time",
        ),
        ("If it is Monday at 08:00 I do the due cold redos.", "then"),
        (
            "Every weekday at 08:00, I do the due cold redos.",
            "If it is",
        ),
    ] {
        let plan = format!("{PLAN}\n## 14. Sessions\n\n- {line}\n");
        let reported = unreadable_in(&plan);
        assert_eq!(reported.len(), 1, "{line}");
        assert!(reported[0].1.contains(part), "{line}: {}", reported[0].1);
    }
}

#[test]
fn line_numbers_count_the_comments_and_code_fences_above_and_inside_the_section() {
    let plan = format!(
        "{PLAN}\n<!-- Old plan:\n## 7. Sessions\n- If it is Tuesday at 06:00, then I run.\n-->\n\n\
             ```markdown\n## 7. Sessions\n```\n\n\
             ## 7. Sessions\n\n<!-- one\ntwo\nthree -->\n```\nfenced\n```\n\
             - If it is Tuesday at 08:00, then I do the due cold redos.\n\
             - If it is Tuesday around eight, then I read.\n"
    );
    let reported = unreadable_in(&plan);
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].0, line_of(&plan, "around eight"));
}
