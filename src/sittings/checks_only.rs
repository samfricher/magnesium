//! A sitting of quick checks alone: the learner asked for the checks that are
//! due and nothing else ("just the checks").
//!
//! The ask is kept beside the sitting it changes, in `checks_alone`, for the
//! conversation that made it: another conversation in the same study folder
//! goes on as in any sitting. It holds until the sitting ends ([`Sittings::end`]
//! removes it), or until that conversation asks for something else: a stage
//! that brings new material, a plan or a paper, or recording off. Code keeps
//! only what it knows for certain: which checks are due, three at most, and
//! when the last of them is settled, which ends the sitting. Everything else
//! is the tutor's to judge.

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
const CLOSE: &str = "Each reply is only what the learner reads: the feedback on their answer, then the next check in its own words. Each answer, a skip included, goes in one learning_step update, with a request_id, before the reply. The runtime keeps these checks and which one is open, so this sitting needs no checkpoint (in the study window, the reply goes without save_after). Write no text before or between tool calls and nothing after your reply, and never a word about posing, saving, recording, declining or waiting. After each answer, show the lettered options, say in a line which is right and why, and pose the next check in the same reply. Teach no repair and set no task for it: a check answered wrong comes back on its next day. When the last check here is answered or declined, Magnesium ends the sitting and shows the learner, under your reply, what their record says of today: do not say it yourself, set no task and ask no question, and close in a line. If that result does not say the sitting ended, send learning_step {\"schema\":\"learner-model/v2\",\"action\":\"end_sitting\"} after the last check. If the learner asks for something else, a lesson, a change to the plan or a paper, give it to them as in any sitting, and load that stage's instructions before you reply: that makes this an ordinary sitting, and the checks not yet answered stay due.";

/// The usual opener's words for the first reply of a sitting; a sitting of
/// checks alone may be asked for part-way through one.
const FIRST_REPLY: &str = "Your first reply of this sitting";

impl Sittings {
    /// The current sitting, while it is one of quick checks alone for the
    /// conversation `by`, and when its opener went to that conversation.
    fn alone(&self, by: Option<&str>) -> Result<Option<(String, Option<i64>)>, String> {
        self.conn
            .query_row(
                "SELECT s.sitting_id, c.shown_ms
                 FROM (SELECT sitting_id FROM sittings ORDER BY sitting_no DESC LIMIT 1) s
                 JOIN checks_alone c ON c.sitting_id = s.sitting_id
                 WHERE c.asked_by IS ?1",
                [by],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(error)
    }

    /// The current sitting, while it is one of quick checks alone for the
    /// conversation `by`.
    pub(crate) fn checks_only(&self, by: Option<&str>) -> Result<Option<String>, String> {
        Ok(self.alone(by)?.map(|(sitting, _)| sitting))
    }

    /// The numbers of the sitting's checks still to settle: none answered or
    /// declined in it, and still due in the record. A check the record no
    /// longer has due (dropped, or answered under another name) is not
    /// waited for.
    fn still_due(
        &self,
        sitting: &str,
        workspace: &Workspace,
        now: i64,
    ) -> Result<Vec<i64>, String> {
        let items = items_now(workspace);
        let due: Vec<String> = crate::practice::due_checks(&items, local_day(now))
            .into_iter()
            .map(|item| item.facts.item_id.clone())
            .collect();
        let mut statement = self
            .conn
            .prepare("SELECT number, item_id FROM sitting_items WHERE sitting_id=?1 AND outcome IS NULL ORDER BY number")
            .map_err(error)?;
        let rows = statement
            .query_map([sitting], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(error)?;
        Ok(rows
            .into_iter()
            .filter(|(_, item)| due.contains(item))
            .map(|(number, _)| number)
            .collect())
    }

    /// How many quick checks a sitting of checks alone has left for the
    /// conversation `by`, while it is one.
    pub(crate) fn checks_only_left(
        &self,
        workspace: &Workspace,
        by: Option<&str>,
        now: i64,
    ) -> Result<Option<i64>, String> {
        let Some(sitting) = self.checks_only(by)? else {
            return Ok(None);
        };
        self.still_due(&sitting, workspace, now)
            .map(|numbers| Some(numbers.len() as i64))
    }

    /// The learner asked for the quick checks alone, at `now`, in the
    /// conversation `by`. The message begins a sitting after the usual gap.
    /// The sitting keeps the checks it took and has not settled, and takes
    /// the checks due today that it does not hold yet, longest due first, up
    /// to [`MAX_CHECKS_PER_SITTING`] in all. (A check answered or declined
    /// today is due again on a later day, so it is not taken twice.) With
    /// any check to pose, the sitting becomes one of checks alone for that
    /// conversation; one asked for later, elsewhere, takes it over. Returns
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
        let mut open = held
            .iter()
            .filter(|(item, open)| *open && due.iter().any(|(id, _)| id == item))
            .count();
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
                "INSERT INTO checks_alone(sitting_id,asked_ms,asked_by,shown_ms) VALUES(?1,?2,?3,NULL)
                 ON CONFLICT(sitting_id) DO UPDATE SET asked_ms=excluded.asked_ms, asked_by=excluded.asked_by, shown_ms=NULL",
                params![sitting, now, by],
            )
            .map_err(error)?;
        }
        tx.commit().map_err(error)?;
        self.write_log();
        Ok(open)
    }

    /// What a sitting of checks alone opens with for the conversation `by`,
    /// as it stands: the checks left to pose, and no paper redos. None when
    /// the sitting is not one for it, or has none left.
    pub(crate) fn checks_only_opener(
        &self,
        workspace: &Workspace,
        by: Option<&str>,
        now: i64,
    ) -> Result<Option<Opener>, String> {
        let Some(sitting) = self.checks_only(by)? else {
            return Ok(None);
        };
        let left = self.still_due(&sitting, workspace, now)?;
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

    /// The opener of a sitting of checks alone, once, for the conversation
    /// that asked: whether or not the sitting's usual opener went out before,
    /// in that conversation or another. It also claims the usual opener, so
    /// the same checks are not posed in another conversation as well.
    pub(crate) fn take_checks_only_opener(
        &mut self,
        workspace: &Workspace,
        by: Option<&str>,
        now: i64,
    ) -> Result<Option<Opener>, String> {
        let Some((sitting, None)) = self.alone(by)? else {
            return Ok(None);
        };
        let Some(opener) = self.checks_only_opener(workspace, by, now)? else {
            return Ok(None);
        };
        let items = items_now(workspace);
        let redos = crate::practice::due_redos(&items, local_day(now));
        let tx = self.write()?;
        tx.execute(
            "UPDATE checks_alone SET shown_ms=?2 WHERE sitting_id=?1",
            params![sitting, now],
        )
        .map_err(error)?;
        tx.execute(
            "UPDATE sittings SET opener_delivered=1, redos_json=?2 WHERE sitting_id=?1 AND opener_delivered=0",
            params![
                sitting,
                json!(redos.iter().map(|r| json!({"gap_log": r.gap_log, "due": r.due, "first": r.first})).collect::<Vec<_>>()).to_string()
            ],
        )
        .map_err(error)?;
        tx.commit().map_err(error)?;
        self.write_log();
        Ok(Some(opener))
    }

    /// The conversation `by` asked for something besides the checks: its
    /// sitting is an ordinary one again, and the checks not yet answered stay
    /// due. A check posed and not answered is set aside, so it is no longer
    /// the open task over the learner's line; a reply that poses it again
    /// opens it again. Returns whether it was one of checks alone.
    pub(crate) fn lift_checks_only(&mut self, by: Option<&str>) -> Result<bool, String> {
        let Some(sitting) = self.checks_only(by)? else {
            return Ok(false);
        };
        let tx = self.write()?;
        tx.execute("DELETE FROM checks_alone WHERE sitting_id=?1", [&sitting])
            .map_err(error)?;
        tx.execute(
            "UPDATE sitting_items SET posed=0 WHERE sitting_id=?1 AND outcome IS NULL",
            [&sitting],
        )
        .map_err(error)?;
        tx.commit().map_err(error)?;
        Ok(true)
    }

    /// End a sitting of checks alone for the conversation `by` once none of
    /// its checks is left to settle. Returns whether this ended it.
    pub(crate) fn end_checks_only_when_done(
        &mut self,
        workspace: &Workspace,
        by: Option<&str>,
        now: i64,
    ) -> Result<bool, String> {
        let Some(sitting) = self.checks_only(by)? else {
            return Ok(false);
        };
        if !self.still_due(&sitting, workspace, now)?.is_empty() {
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
