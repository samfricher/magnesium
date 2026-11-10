//! Reading the learner's plan and gap log, which are Markdown the tutor wrote.
//!
//! The tutor edits these files by hand, so every reader here is tolerant: a
//! column that is missing gives an empty cell, and a row that cannot be read is
//! counted and reported, never guessed at.

use chrono::NaiveDate;
use serde::Serialize;

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// The first column whose name matches one of `names`, without regard to
    /// case, spaces or hyphens.
    pub fn column(&self, names: &[&str]) -> Option<usize> {
        let plain = |text: &str| {
            text.chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        };
        names.iter().find_map(|name| {
            let wanted = plain(name);
            self.header.iter().position(|cell| plain(cell) == wanted)
        })
    }

    pub fn cell<'a>(&self, row: &'a [String], names: &[&str]) -> &'a str {
        self.column(names)
            .and_then(|index| row.get(index))
            .map(|cell| cell.trim())
            .filter(|cell| !is_placeholder(cell))
            .unwrap_or("")
    }
}

/// A cell the template left unfilled, such as `<TAG>`.
pub fn is_placeholder(cell: &str) -> bool {
    let cell = cell.trim();
    cell.is_empty() || (cell.starts_with('<') && cell.ends_with('>'))
}

/// The text under the heading `## name`, up to the next heading of that level.
/// A heading may carry a number, as in `## 1. Facts`.
pub fn section<'a>(text: &'a str, name: &str) -> &'a str {
    let mut start = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if let Some(from) = start {
                return &text[from..offset];
            }
            let heading = heading
                .trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ')
                .trim();
            if heading.eq_ignore_ascii_case(name) {
                start = Some(offset + line.len());
            }
        }
        offset += line.len();
    }
    start.map_or("", |from| &text[from..])
}

fn split_cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                current.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut current).trim().to_owned()),
            other => current.push(other),
        }
    }
    cells.push(current.trim().to_owned());
    cells
}

fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| matches!(c, '-' | ':' | ' ')))
}

/// The first table in a piece of Markdown.
pub fn table(text: &str) -> Table {
    let mut header: Option<Vec<String>> = None;
    let mut rows = Vec::new();
    let mut in_comment = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if in_comment {
            in_comment = !trimmed.contains("-->");
            continue;
        }
        if trimmed.starts_with("<!--") {
            in_comment = !trimmed.contains("-->");
            continue;
        }
        if !trimmed.starts_with('|') {
            if header.is_some() && !rows.is_empty() {
                break;
            }
            continue;
        }
        let cells = split_cells(trimmed);
        if is_separator(&cells) {
            continue;
        }
        match &header {
            None => header = Some(cells),
            Some(_) => rows.push(cells),
        }
    }
    Table {
        header: header.unwrap_or_default(),
        rows,
    }
}

pub fn first_date(text: &str) -> Option<NaiveDate> {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(9)).find_map(|start| {
        let slice = text.get(start..start + 10)?;
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_digit();
        let after_ok = bytes.get(start + 10).is_none_or(|b| !b.is_ascii_digit());
        (before_ok && after_ok)
            .then(|| NaiveDate::parse_from_str(slice, "%Y-%m-%d").ok())
            .flatten()
    })
}

fn first_number(text: &str) -> Option<u32> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

/// One row of the gap log: one lost mark and the proof that it was repaired.
#[derive(Clone, Debug, Serialize)]
pub struct Gap {
    pub paper: String,
    pub question: String,
    pub tag: String,
    pub wrong: String,
    pub confidence: String,
    /// Unaided passes on separate days since the last failure, 0 to 3.
    pub passes: u8,
    /// Passes made before a later failure. They are kept as history and do
    /// not count.
    pub earlier_passes: u8,
    pub failed_on: Option<NaiveDate>,
    pub last_pass: Option<NaiveDate>,
    pub next_redo: Option<NaiveDate>,
    pub closed: bool,
    pub repaired: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Score {
    pub paper: String,
    pub date: String,
    pub score: String,
    /// Right answers out of answers marked "sure", when the notes give them.
    pub sure_right: Option<u32>,
    pub sure_total: Option<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct GapLog {
    pub gaps: Vec<Gap>,
    pub scores: Vec<Score>,
    /// Rows of the Gaps table that could not be read.
    pub unreadable: usize,
}

fn status_passes(status: &str) -> Option<(u8, bool)> {
    let status = status.trim().to_ascii_lowercase();
    if status.starts_with("closed") || status.starts_with("3 of 3") {
        Some((3, true))
    } else if status.starts_with("2 of 3") {
        Some((2, false))
    } else if status.starts_with("1 of 3") {
        Some((1, false))
    } else if status.starts_with("open") {
        Some((0, false))
    } else {
        None
    }
}

/// The dated passes and failures in a Passes cell, in the order written.
/// The runtime writes each as `2026-11-05 pass` or `2026-11-08 fail`, and a
/// pass in another paper as `2026-11-05 pass (MockA Q3)`. The word that comes
/// first decides, so a paper whose name holds "fail" cannot turn a pass into
/// a failure.
fn pass_entries(cell: &str) -> Vec<(Option<NaiveDate>, bool)> {
    cell.split([';', ','])
        .flat_map(|part| part.split("<br>"))
        .filter_map(|part| {
            let lower = part.to_ascii_lowercase();
            let passed = match (lower.find("pass"), lower.find("fail")) {
                (Some(pass), Some(fail)) => pass < fail,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => return None,
            };
            Some((first_date(part), passed))
        })
        .collect()
}

pub fn read_gap_log(text: &str) -> GapLog {
    let gaps_table = table(section(text, "Gaps"));
    let mut log = GapLog::default();
    for row in &gaps_table.rows {
        let cell = |names: &[&str]| gaps_table.cell(row, names).to_owned();
        let tag = cell(&["Tag"]);
        let wrong = cell(&["What went wrong", "What happened", "What"]);
        let status = cell(&["Status"]);
        if tag.is_empty() && wrong.is_empty() {
            // The template's example row, or an empty line.
            continue;
        }
        let entries = pass_entries(&cell(&["Passes"]));
        let last_fail = entries.iter().rposition(|(_, passed)| !passed);
        let since = last_fail.map_or(0, |index| index + 1);
        let counted = entries[since..].len().min(3) as u8;
        let earlier = last_fail.map_or(0, |index| {
            entries[..index]
                .iter()
                .filter(|(_, passed)| *passed)
                .count()
                .min(3) as u8
        });
        let Some((passes, closed)) = status_passes(&status).or_else(|| {
            // An older log may have no Status column a reader can use. The
            // dated passes are then the only evidence.
            (!entries.is_empty() || status.is_empty()).then_some((counted, counted >= 3))
        }) else {
            log.unreadable += 1;
            continue;
        };
        let repair = cell(&["Repair done", "Repair"]);
        log.gaps.push(Gap {
            paper: cell(&["Paper"]),
            question: cell(&["Q", "Question"]),
            tag,
            wrong,
            confidence: cell(&["Conf", "Confidence"]),
            passes,
            earlier_passes: earlier.min(3 - passes.min(3)),
            failed_on: last_fail.and_then(|index| entries[index].0),
            last_pass: entries[since..].iter().rev().find_map(|(date, _)| *date),
            next_redo: first_date(&cell(&[
                "Next redo",
                "Cold-redo date",
                "Cold redo date",
                "Redo",
            ])),
            closed,
            repaired: !repair.is_empty(),
        });
    }

    let scores_table = table(section(text, "Scores"));
    for row in &scores_table.rows {
        let paper = scores_table.cell(row, &["Paper"]).to_owned();
        let score = scores_table.cell(row, &["Score"]).to_owned();
        if paper.is_empty() || score.is_empty() {
            continue;
        }
        let notes = scores_table.cell(row, &["Notes"]);
        let (sure_right, sure_total) = sure_count(notes).unzip();
        log.scores.push(Score {
            paper,
            date: scores_table.cell(row, &["Date"]).to_owned(),
            score,
            sure_right,
            sure_total,
        });
    }
    log
}

/// "Conf right: S 8/11, U 4/6" gives (8, 11).
fn sure_count(notes: &str) -> Option<(u32, u32)> {
    const LABEL: &str = "conf right";
    // Lower-casing ASCII keeps every byte offset, so the offset holds in `notes`.
    let at = notes.to_ascii_lowercase().find(LABEL)?;
    let chars: Vec<char> = notes[at + LABEL.len()..].chars().collect();
    let s = (0..chars.len()).find(|&i| {
        chars[i] == 'S'
            && chars[i + 1..]
                .iter()
                .find(|c| !c.is_whitespace())
                .is_some_and(char::is_ascii_digit)
    })?;
    let tail: String = chars[s + 1..].iter().collect();
    let (right, total) = tail.split_once('/')?;
    Some((first_number(right)?, first_number(total)?))
}

/// The plan's facts the window shows.
#[derive(Clone, Debug, Default)]
pub struct PlanFacts {
    pub minutes: Option<u32>,
    /// Minutes for each named paper, from the provenance table.
    pub paper_minutes: Vec<(String, u32)>,
}

pub fn read_plan(text: &str) -> PlanFacts {
    let facts = table(section(text, "Facts"));
    let minutes = facts
        .rows
        .iter()
        .find(|row| facts.cell(row, &["Item"]).eq_ignore_ascii_case("Length"))
        .and_then(|row| first_number(facts.cell(row, &["Value"])));
    // The provenance table sits under a third-level heading inside "Paper bank".
    let bank = section(text, "Paper bank");
    let provenance = bank
        .find("### Provenance")
        .map(|at| table(&bank[at..]))
        .unwrap_or_default();
    let paper_minutes = provenance
        .rows
        .iter()
        .filter_map(|row| {
            let name = provenance.cell(row, &["Paper"]);
            let time = first_number(provenance.cell(row, &["Time"]))?;
            (!name.is_empty()).then(|| (name.to_owned(), time))
        })
        .collect();
    PlanFacts {
        minutes,
        paper_minutes,
    }
}

/// The course's tags and what each covers, from `graph.yaml`. The file is
/// simple enough to read by its indentation, so no YAML library is needed.
pub fn read_tags(text: &str) -> Vec<(String, String)> {
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if indent == 0 {
            inside = trimmed == "tags:";
            continue;
        }
        if !inside {
            continue;
        }
        if indent == 2 && trimmed.ends_with(':') {
            let name = trimmed.trim_end_matches(':').trim_matches(['"', '\'']);
            if !is_placeholder(name) {
                tags.push((name.to_owned(), String::new()));
            }
        } else if indent > 2 {
            if let (Some(last), Some(covers)) = (tags.last_mut(), trimmed.strip_prefix("covers:")) {
                let covers = covers.trim().trim_matches(['"', '\'']);
                if !is_placeholder(covers) {
                    last.1 = covers.to_owned();
                }
            }
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "# Gap log: midterm-1\n\n## Gaps\n\n\
| # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |\n\
|---|---|---|---|---|---|---|---|---|---|---|---|\n\
| 1 | 2026-11-01 | Diagnostic | Q1 | KM | read Km as the intercept | K | S | taught | 2026-11-02 pass; 2026-11-05 pass; 2026-11-12 pass | | closed |\n\
| 2 | 2026-11-01 | Diagnostic | Q2 | VMAX | took the slope for Vmax | K | U | repaired | 2026-11-02 pass; 2026-11-05 pass | 2026-11-14 | 2 of 3 |\n\
| 3 | 2026-11-01 | Diagnostic | Q3 | SLOPE | lost the sign of the slope | K | S | repaired | 2026-11-02 pass; 2026-11-05 pass; 2026-11-08 fail | | open |\n\
| 4 | 2026-11-06 | Paper A | Q5 | KCAT \\| KM | divided by the wrong total | N | | | | | open |\n\
| 5 | <YYYY-MM-DD> | <PAPER> | <Q> | <TAG> | <what> | <K> | <S> | <r> | <e> | <e> | open |\n\
| 6 | 2026-11-06 | Paper A | Q6 | HILL | read n from the wrong axis | K | G | repaired | | 2026-11-10 | sort of |\n\n\
## Scores\n\n\
| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n\
| Diagnostic | 2026-11-01 | 15/30 | 50/50 min | Q1 2/5. Conf right: S 8/11, U 4/6, G 1/2 |\n\
| Paper A | 2026-11-06 | 20/30 | 44/50 min | no letters |\n";

    #[test]
    fn rows_are_read_with_their_passes() {
        let log = read_gap_log(LOG);
        assert_eq!(log.gaps.len(), 4, "the template row is skipped");
        let closed = &log.gaps[0];
        assert!(closed.closed);
        assert_eq!(closed.passes, 3);
        assert_eq!(closed.last_pass, NaiveDate::from_ymd_opt(2026, 11, 12));
        let two = &log.gaps[1];
        assert_eq!((two.passes, two.closed), (2, false));
        assert_eq!(two.next_redo, NaiveDate::from_ymd_opt(2026, 11, 14));
    }

    #[test]
    fn a_failed_redo_keeps_its_earlier_passes_as_history() {
        let log = read_gap_log(LOG);
        let failed = &log.gaps[2];
        assert_eq!(failed.passes, 0);
        assert_eq!(failed.earlier_passes, 2);
        assert_eq!(failed.failed_on, NaiveDate::from_ymd_opt(2026, 11, 8));
    }

    #[test]
    fn an_escaped_bar_stays_inside_its_cell() {
        let log = read_gap_log(LOG);
        assert_eq!(log.gaps[3].tag, "KCAT | KM");
        assert!(!log.gaps[3].repaired);
    }

    #[test]
    fn a_status_nobody_can_read_is_counted_and_not_guessed() {
        let log = read_gap_log(LOG);
        assert!(log.gaps.iter().all(|g| g.tag != "HILL"));
        assert_eq!(log.unreadable, 1);
    }

    #[test]
    fn scores_carry_the_sure_count() {
        let log = read_gap_log(LOG);
        assert_eq!(log.scores.len(), 2);
        assert_eq!(log.scores[0].sure_right, Some(8));
        assert_eq!(log.scores[0].sure_total, Some(11));
        assert_eq!(log.scores[1].sure_total, None);
    }

    #[test]
    fn an_older_log_without_a_next_redo_column_still_reads() {
        let old = "## Gaps\n\n| # | Paper | Q | Tag | What went wrong | Cold-redo date | Status |\n|---|---|---|---|---|---|---|\n| 1 | Diag | Q1 | T1 | wrong sign | 2026-10-20 | open |\n";
        let log = read_gap_log(old);
        assert_eq!(log.gaps.len(), 1);
        assert_eq!(log.gaps[0].next_redo, NaiveDate::from_ymd_opt(2026, 10, 20));
    }

    #[test]
    fn a_pass_in_a_paper_named_for_failure_is_still_a_pass() {
        let entries =
            pass_entries("2026-11-02 pass, 2026-11-05 pass (Fail-safe Q3), 2026-11-12 fail");
        assert_eq!(
            entries
                .iter()
                .map(|(_, passed)| *passed)
                .collect::<Vec<_>>(),
            vec![true, true, false]
        );
    }

    #[test]
    fn plan_facts_give_the_length() {
        let plan = "# X\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| Length | 50 minutes |\n\n## 4. Paper bank\n\n### Provenance\n\n| Paper | Level | Time | Marks |\n|---|---|---|---|\n| Paper B | hard | 45 | 30 |\n\n## 5. Method\n";
        let facts = read_plan(plan);
        assert_eq!(facts.minutes, Some(50));
        assert_eq!(facts.paper_minutes, vec![("Paper B".to_owned(), 45)]);
    }

    #[test]
    fn tags_are_read_from_the_graph() {
        let graph = "# c\ntags:\n  KM:\n    covers: reading Km from a plot\n    kind: course\n  \"V MAX\":\n    covers: \"Vmax: the top speed\"\n  <TAG>:\n    covers: <x>\nother:\n  X:\n";
        assert_eq!(
            read_tags(graph),
            vec![
                ("KM".to_owned(), "reading Km from a plot".to_owned()),
                ("V MAX".to_owned(), "Vmax: the top speed".to_owned()),
            ]
        );
    }
}
