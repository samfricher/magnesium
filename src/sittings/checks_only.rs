//! A sitting of quick checks alone: the learner asked for the checks that are
//! due and nothing else ("just the checks").
//!
//! The ask is kept beside the sitting it changes, in `sitting_checks_only`,
//! and holds until that sitting ends: [`Sittings::end`] removes it, so a
//! sitting carried on after its end is not checks alone again unless asked.
//! While it holds, the sitting's opener is its quick checks alone, with no
//! paper redos, and the runtime ends the sitting itself once each of them is
//! answered or declined.

use rusqlite::{OptionalExtension, params};
use serde_json::json;

use super::{
    Check, MAX_CHECKS_PER_SITTING, Opener, Sittings, checks_json, error, items_now, local_day,
    opener_text,
};
use crate::workspace::Workspace;

/// What the tutor reads first, in place of the usual opener's first line.
const LEAD: &str = "Quick checks alone. The learner asked for the quick checks that are due and nothing else, so this sitting poses only these, one at a time: no paper redo, no new material, no plan and no other task. The learner may decline any check at no cost, and a declined check stays due.";
/// What the tutor reads last: how the sitting ends.
const CLOSE: &str = "Each reply is only what the learner reads: the feedback on their answer, then the next check in its own words. Each answer, a skip included, goes in one learning_step update, with a request_id, before the reply. The runtime keeps these checks and which one is open, so this sitting needs no checkpoint (in the study window, the reply goes without save_after). Write no text before or between tool calls and nothing after your reply, and never a word about posing, saving, recording, declining or waiting. After each answer, show the lettered options, say in a line which is right and why, and pose the next check in the same reply. Teach no repair and set no task for it: a check answered wrong comes back on its next day. When the last check here is answered or declined, Magnesium ends the sitting and shows the learner, under your reply, what their record says of today: do not say it yourself, set no task and ask no question, and close in a line. If the learner asks for something else instead, each check not yet answered counts as declined, and what they asked comes next.";

/// The usual opener's words for the first reply of a sitting; a sitting of
/// checks alone may be asked for part-way through one.
const FIRST_REPLY: &str = "Your first reply of this sitting";

impl Sittings {
    /// The current sitting, while it is one of quick checks alone: from the
    /// ask until it ends ([`Sittings::end`] removes the ask).
    pub(crate) fn checks_only(&self) -> Result<Option<String>, String> {
        self.conn
            .query_row(
                "SELECT s.sitting_id
                 FROM (SELECT sitting_id FROM sittings ORDER BY sitting_no DESC LIMIT 1) s
                 JOIN sitting_checks_only c ON c.sitting_id = s.sitting_id",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(error)
    }

    /// How many quick checks a sitting of checks alone has left to answer or
    /// decline, while it is one.
    pub(crate) fn checks_only_left(&self) -> Result<Option<i64>, String> {
        let Some(sitting) = self.checks_only()? else {
            return Ok(None);
        };
        self.unsettled(&sitting)
            .map(|numbers| Some(numbers.len() as i64))
    }

    fn unsettled(&self, sitting: &str) -> Result<Vec<i64>, String> {
        let mut statement = self
            .conn
            .prepare("SELECT number FROM sitting_items WHERE sitting_id=?1 AND outcome IS NULL ORDER BY number")
            .map_err(error)?;
        statement
            .query_map([sitting], |r| r.get(0))
            .map_err(error)?
            .collect::<rusqlite::Result<Vec<i64>>>()
            .map_err(error)
    }

    /// The learner asked for the quick checks alone, at `now`, in the
    /// conversation `by`. The message begins a sitting after the usual gap.
    /// The sitting keeps the checks it took and has not settled, and takes
    /// the checks due today that it does not hold yet, longest due first, up
    /// to [`MAX_CHECKS_PER_SITTING`] in all. (A check answered or declined
    /// today is due again on a later day, so it is not taken twice.) With
    /// any check to pose, the sitting becomes one of checks alone. Returns
    /// how many it holds to pose: none when no check is due.
    pub(crate) fn ask_checks_only(
        &mut self,
        workspace: &Workspace,
        by: Option<&str>,
        now: i64,
    ) -> Result<usize, String> {
        self.note_learner_message(workspace, now)?;
        let Some((sitting, _)) = self.current()? else {
            return Ok(0);
        };
        let items = items_now(workspace);
        let due: Vec<(String, String)> = crate::practice::due_checks(&items, local_day(now))
            .into_iter()
            .map(|item| (item.facts.item_id.clone(), item.facts.label.clone()))
            .collect();
        let tx = self.write()?;
        let held: Vec<(String, bool)> = {
            let mut statement = tx
                .prepare("SELECT item_id, outcome IS NULL FROM sitting_items WHERE sitting_id=?1")
                .map_err(error)?;
            statement
                .query_map([&sitting], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(error)?
                .collect::<rusqlite::Result<_>>()
                .map_err(error)?
        };
        let mut open = held.iter().filter(|(_, open)| *open).count();
        let mut number: i64 = tx
            .query_row(
                "SELECT coalesce(max(number), 0) FROM sitting_items WHERE sitting_id=?1",
                [&sitting],
                |r| r.get(0),
            )
            .map_err(error)?;
        for (item, label) in &due {
            if open >= MAX_CHECKS_PER_SITTING {
                break;
            }
            if held.iter().any(|(id, _)| id == item) {
                continue;
            }
            number += 1;
            tx.execute(
                "INSERT INTO sitting_items(sitting_id,item_id,number,label) VALUES(?1,?2,?3,?4)",
                params![sitting, item, number, label],
            )
            .map_err(error)?;
            open += 1;
        }
        if open > 0 {
            tx.execute(
                "INSERT INTO sitting_checks_only(sitting_id,asked_ms,asked_by) VALUES(?1,?2,?3)
                 ON CONFLICT(sitting_id) DO UPDATE SET asked_ms=excluded.asked_ms, asked_by=excluded.asked_by",
                params![sitting, now, by],
            )
            .map_err(error)?;
        }
        tx.commit().map_err(error)?;
        self.write_log();
        Ok(open)
    }

    /// What a sitting of checks alone opens with, as it stands: the checks
    /// left to pose, and no paper redos. None when the sitting is not one,
    /// or has none left.
    pub(crate) fn checks_only_opener(
        &self,
        workspace: &Workspace,
    ) -> Result<Option<Opener>, String> {
        let Some(sitting) = self.checks_only()? else {
            return Ok(None);
        };
        let left = self.unsettled(&sitting)?;
        let items = items_now(workspace);
        let checks: Vec<Check> = self
            .checks(&sitting, &items)?
            .into_iter()
            .filter(|check| left.contains(&check.number))
            .collect();
        Ok((!checks.is_empty()).then(|| Opener {
            text: checks_only_text(&checks),
            checks: checks_json(&checks),
            redos: json!([]),
        }))
    }

    /// End a sitting of checks alone once each of its checks is answered or
    /// declined, for the conversation `by`. Returns whether this ended it.
    pub(crate) fn end_checks_only_when_done(
        &mut self,
        by: Option<&str>,
        now: i64,
    ) -> Result<bool, String> {
        let Some(sitting) = self.checks_only()? else {
            return Ok(false);
        };
        if !self.unsettled(&sitting)?.is_empty() {
            return Ok(false);
        }
        Ok(self.end(by, now)?.is_some())
    }
}

/// The opener of a sitting of checks alone: the usual opener's checks and how
/// to pose and record them, between this sitting's first and last words.
fn checks_only_text(checks: &[Check]) -> String {
    let opener = opener_text(checks, &[]);
    let body = opener
        .split_once("\n\n")
        .map_or(opener.as_str(), |(_, body)| body)
        .replacen(FIRST_REPLY, "Your next reply", 1);
    format!("{LEAD}\n\n{body}\n\n{CLOSE}")
}

/// The clause each turn's notice carries while a sitting of checks alone has
/// checks left. It states the count and asks nothing. In the live runs of
/// 2026-11-16, Sonnet wrote no process text in an ordinary sitting, whose
/// clause only counts, and wrote "Record decline." into the reply in this
/// flow while its clause gave an instruction.
pub(crate) fn notice_clause(left: i64) -> String {
    if left > 0 {
        format!(" Sitting of quick checks alone: {left} left.")
    } else {
        " Sitting of quick checks alone: none left.".to_owned()
    }
}

#[cfg(test)]
#[path = "checks_only_tests.rs"]
mod tests;
