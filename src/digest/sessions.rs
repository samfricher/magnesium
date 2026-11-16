//! The plan's Sessions section: if-then lines that say when the learner practises.
use super::{Unreadable, section};
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

/// What the plan's Sessions section gives: the start times of the sessions that
/// fall today, and a note for each line it could not read.
pub(super) struct PlannedSessions {
    pub(super) today: Vec<String>,
    pub(super) unreadable: Vec<Unreadable>,
}

/// One line of the Sessions section, as the digest classifies it.
enum Line {
    Blank,
    /// A line that holds no session: a backup, "none planned" or the template's own.
    Other,
    Session(WeekdaySet, NaiveTime),
    Unreadable(&'static str),
}

const NOT_A_SESSION: &str = "the line is not a session (\"If it is <days> at HH:MM, then I ...\"), a backup (\"If I miss ...\") or \"none planned\"";
const NO_THEN: &str = "the line has no \", then I ...\" after the days and start time";
const BAD_DAYS: &str =
    "the days are not day names, a range such as Monday to Friday, a list, weekdays or the weekend";
const BAD_TIME: &str = "the start time is not a 24-hour HH:MM such as 08:00";

pub(super) fn read(file: &str, plan: &str, today: NaiveDate) -> PlannedSessions {
    let rendered = rendered(plan);
    let sessions = section(&rendered, "Sessions");
    let mut found = PlannedSessions {
        today: Vec::new(),
        unreadable: Vec::new(),
    };
    for (index, text) in sessions.text.lines().enumerate() {
        match classify(text) {
            Line::Blank | Line::Other => {}
            Line::Session(days, start) => {
                if days.contains(today.weekday()) {
                    found.today.push(start.format("%H:%M").to_string());
                }
            }
            Line::Unreadable(reason) => found.unreadable.push(Unreadable::at(
                file,
                "Sessions",
                sessions.first_line + index,
                reason.to_owned(),
            )),
        }
    }
    found.today.sort();
    found.today.dedup();
    found
}

fn classify(line: &str) -> Line {
    let line = line
        .replace(['*', '_'], "")
        .replace(['–', '—'], "-")
        .to_lowercase();
    let line = line.trim().trim_start_matches('-').trim_start();
    if line.is_empty() {
        Line::Blank
    } else if line.trim_end_matches('.') == "none planned"
        || line.starts_with("if i miss")
        || line.contains("<hh:mm>")
    {
        Line::Other
    } else {
        match planned_session(line) {
            Ok((days, start)) => Line::Session(days, start),
            Err(reason) => Line::Unreadable(reason),
        }
    }
}

/// The text with each HTML comment taken out. The comment's line breaks stay,
/// so a line keeps its number in the file.
fn without_comments(text: &str) -> String {
    let mut kept = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        kept.push_str(&rest[..start]);
        let (hidden, after) = rest[start..]
            .split_once("-->")
            .map_or((&rest[start..], ""), |(hidden, after)| (hidden, after));
        kept.extend(hidden.chars().filter(|c| *c == '\n'));
        rest = after;
    }
    kept + rest
}

/// The text as Markdown shows it: comments and fenced code are blank lines.
fn rendered(text: &str) -> String {
    let text = without_comments(text);
    let mut fence = None;
    text.lines()
        .map(|line| {
            let marker = ["```", "~~~"]
                .into_iter()
                .find(|marker| line.trim_start().starts_with(marker));
            let shown = match (fence, marker) {
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
            };
            format!("{}\n", if shown { line } else { "" })
        })
        .collect()
}

/// `line` is lowercase, with its list marker and emphasis marks taken off.
fn planned_session(line: &str) -> Result<(WeekdaySet, NaiveTime), &'static str> {
    let cue = ["if it is ", "if it's ", "if it\u{2019}s "]
        .iter()
        .find_map(|start| line.strip_prefix(start))
        .ok_or(NOT_A_SESSION)?;
    let (cue, _) = cue.split_once(" then ").ok_or(NO_THEN)?;
    let cue = cue.trim_end_matches(',');
    let (days, time) = match cue.rsplit_once(" at ") {
        Some((days, time)) => (day_set(days).ok_or(BAD_DAYS)?, time),
        None => (WeekdaySet::ALL, cue),
    };
    Ok((days, start_time(time.trim()).ok_or(BAD_TIME)?))
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
const UNREADABLE_SESSIONS: &str = "- If it is Monday to Friday except Wednesday at 08:00, then I do the due cold redos.\n\
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
mod tests;
