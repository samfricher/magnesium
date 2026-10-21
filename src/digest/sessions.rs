//! The plan's Sessions section: if-then lines that say when the learner practises.
use super::section;
use chrono::{Datelike, NaiveDate, NaiveTime, Weekday, WeekdaySet};

const DAY_NAMES: [(&str, Weekday); 7] = [
    ("monday", Weekday::Mon),
    ("tuesday", Weekday::Tue),
    ("wednesday", Weekday::Wed),
    ("thursday", Weekday::Thu),
    ("friday", Weekday::Fri),
    ("saturday", Weekday::Sat),
    ("sunday", Weekday::Sun),
];
const WEEKDAYS: WeekdaySet = WeekdaySet::from_array([
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
]);
const WEEKEND: WeekdaySet = WeekdaySet::from_array([Weekday::Sat, Weekday::Sun]);

/// Start times, earliest first, of the planned sessions in `plan` that fall on `today`.
pub(super) fn planned_today(plan: &str, today: NaiveDate) -> Vec<String> {
    let rendered = rendered(plan);
    planned_sessions_today(section(&rendered, "Sessions").text, today)
}

fn planned_sessions_today(text: &str, today: NaiveDate) -> Vec<String> {
    let mut times = text
        .lines()
        .filter_map(planned_session)
        .filter(|(days, _)| days.contains(today.weekday()))
        .map(|(_, time)| time.format("%H:%M").to_string())
        .collect::<Vec<_>>();
    times.sort();
    times.dedup();
    times
}

fn without_comments(text: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        rest = rest[start..]
            .split_once("-->")
            .map_or("", |(_, after)| after);
    }
    kept + rest
}

fn rendered(text: &str) -> String {
    let text = without_comments(text);
    let mut fence = None;
    text.lines()
        .filter(|line| {
            let marker = ["```", "~~~"]
                .into_iter()
                .find(|marker| line.trim_start().starts_with(marker));
            match (fence, marker) {
                (None, None) => true,
                (None, Some(_)) => {
                    fence = marker;
                    false
                }
                (Some(open), Some(close)) if open == close => {
                    fence = None;
                    false
                }
                (Some(_), _) => false,
            }
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

fn planned_session(line: &str) -> Option<(WeekdaySet, NaiveTime)> {
    let line = line
        .replace(['*', '_'], "")
        .replace(['–', '—'], "-")
        .to_lowercase();
    let line = line.trim().trim_start_matches('-').trim_start();
    let cue = ["if it is ", "if it's ", "if it’s "]
        .iter()
        .find_map(|start| line.strip_prefix(start))?;
    let (cue, _) = cue.split_once(" then ")?;
    let cue = cue.trim_end_matches(',');
    let (days, time) = match cue.rsplit_once(" at ") {
        Some((days, time)) => (day_set(days)?, time),
        None => (WeekdaySet::ALL, cue),
    };
    Some((days, start_time(time.trim())?))
}

fn start_time(text: &str) -> Option<NaiveTime> {
    match text.split_once(" to ").or_else(|| text.split_once('-')) {
        Some((start, end)) => clock(end.trim()).and(clock(start.trim())),
        None => clock(text),
    }
}

fn clock(text: &str) -> Option<NaiveTime> {
    let (hour, minute) = text.split_once(':')?;
    let digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
    if hour.len() > 2 || minute.len() != 2 || !digits(hour) || !digits(minute) {
        return None;
    }
    NaiveTime::from_hms_opt(hour.parse().ok()?, minute.parse().ok()?, 0)
}

fn day_set(phrase: &str) -> Option<WeekdaySet> {
    let mut days = WeekdaySet::EMPTY;
    for item in phrase
        .split([',', '/', '&'])
        .flat_map(|part| part.split(" and "))
        .flat_map(|part| part.split(" or "))
    {
        let item = item.trim();
        let item = ["every ", "each "]
            .iter()
            .find_map(|start| item.strip_prefix(start))
            .unwrap_or(item);
        days = days.union(match item {
            "" => WeekdaySet::EMPTY,
            "day" | "any day" | "daily" | "everyday" => WeekdaySet::ALL,
            "weekday" | "weekdays" | "a weekday" => WEEKDAYS,
            "weekend" | "weekends" | "the weekend" => WEEKEND,
            _ => match item
                .split_once(" to ")
                .or_else(|| item.split_once(" through "))
                .or_else(|| item.split_once('-'))
            {
                Some((first, last)) => day_range(weekday(first.trim())?, weekday(last.trim())?),
                None => WeekdaySet::single(weekday(item)?),
            },
        });
    }
    (!days.is_empty()).then_some(days)
}

fn weekday(word: &str) -> Option<Weekday> {
    let named = |word: &str| {
        DAY_NAMES
            .iter()
            .find(|(name, _)| word.len() >= 3 && name.starts_with(word))
            .map(|(_, day)| *day)
    };
    named(word).or_else(|| named(word.strip_suffix('s')?))
}

fn day_range(first: Weekday, last: Weekday) -> WeekdaySet {
    std::iter::successors(Some(first), |day| (*day != last).then(|| day.succ())).collect()
}

#[cfg(test)]
pub(super) const UNREADABLE_SESSIONS: &str = "- If it is Monday to Friday except Wednesday at 08:00, then I do the due cold redos.\n\
    - If it is a weekday at 8am, then I do the due cold redos.\n\
    - If it is Monday at 7:30 pm, then I do the due cold redos.\n\
    - If it is Monday at 25:00, then I do the due cold redos.\n\
    - If it is Funday at 08:00, then I do the due cold redos.\n\
    - If it is Monday or when I can at 08:00, then I do the due cold redos.\n\
    - If it is 08:00 on Mondays, then I do the due cold redos.\n\
    - If it is Monday at 08:00 before class, then I do the due cold redos.\n\
    - If it is Monday to Friday at 08:00 I do the due cold redos.\n\
    - Every weekday at 08:00, I do the due cold redos.\n\
    - If I miss Monday at 08:00, then I do them after dinner.\n\
    - If it is <days> at <HH:MM>, then I <what I do> <where>.\n\
    - If it is T at 08:00, then I do the due cold redos.\n\
    - If it is Monday at 8:5, then I do the due cold redos.\n\
    - If it is Monday at 08:5, then I do the due cold redos.\n\
    - If it is Monday at 008:00, then I do the due cold redos.\n\
    - If it is Monday at +8:00, then I do the due cold redos.\n\
    - If it is Monday at 08:00 to dinner, then I do the due cold redos.\n";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const PLAN: &str = "# Midterm 1: study plan\n\n## 6. Blocks\n\n| # | Block | Purpose |\n|---|---|---|\n| 1 | Diagnostic | find gaps |\n\n## 7. Must know cold\n\n- Km\n";

    fn sessions_on(lines: &str, day: Weekday) -> Value {
        let plan = format!("{PLAN}\n## 14. Sessions\n\n{lines}\n");
        let today = NaiveDate::from_isoywd_opt(2026, 43, day).unwrap();
        json!(planned_today(&plan, today))
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
        assert_eq!(json!(planned_today(&plan, tuesday)), json!(["08:00"]));
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
        assert_eq!(json!(planned_today(&plan, today)), json!([]));
        assert_eq!(
            sessions_on(
                &format!("If it is 08:00, then I do the due cold redos.\n{checklist}"),
                Weekday::Tue
            ),
            json!(["08:00"])
        );
    }
}
