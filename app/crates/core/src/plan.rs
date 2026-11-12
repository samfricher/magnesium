//! The open folder's study plan, as the window shows it: the exam it is for,
//! the goal being worked on and its steps, and the plan's own list of what to
//! study. It is read fresh each time from the runtime's digest, the record's
//! last checkpoint and the plan file, and never kept by the window.
//!
//! After each turn the manager reads it again and tells the page what
//! changed, so a screen that shows the plan follows the tutor's edits without
//! polling.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::peek::Step;

/// One thing on the plan's list of what to study, as the plan file holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    /// Stable while the item's title is: the plan's own number when it has
    /// one, else a slug of the title.
    pub id: String,
    pub title: String,
    /// "todo", "doing" or "done".
    pub status: String,
    /// The day the plan puts it on, as "2026-11-14", when it names one.
    pub day: Option<String>,
    /// The plan's own words about it, such as its purpose or length.
    pub note: Option<String>,
}

/// The open folder's study plan. Every field is empty when the folder has
/// none: a folder with no plan is not behind on anything.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// The plan's key, the name of its folder under `Study/`: "midterm-2".
    pub target: Option<String>,
    /// "Midterm 2".
    pub title: Option<String>,
    /// "2026-11-20", or none when the plan has no date.
    pub exam_date: Option<String>,
    /// The goal being worked on, from the record's last session.
    pub goal: Option<String>,
    /// That goal's route, from the record's last checkpoint.
    pub steps: Vec<Step>,
    /// The plan's ordered list of what to study.
    pub items: Vec<PlanItem>,
    /// The plan file's Markdown, for a screen that shows it as a document.
    pub text: Option<String>,
    /// The plan file, relative to the study root: "Study/midterm-2/plan.md".
    pub path: Option<String>,
    /// When any of the above last changed, in ms since the epoch.
    pub changed_ms: Option<i64>,
}

/// One part of the plan that a turn changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanPart {
    ExamDate,
    Items,
    Goal,
    Steps,
    Text,
}

/// The open folder's plan, read fresh.
pub fn read(_folder: &Path) -> Plan {
    Plan::default()
}

/// What differs between two readings of the plan, in a fixed order. Empty
/// when nothing does.
pub fn changes(before: &Plan, after: &Plan) -> Vec<PlanPart> {
    let mut what = Vec::new();
    if before.exam_date != after.exam_date {
        what.push(PlanPart::ExamDate);
    }
    if before.items != after.items {
        what.push(PlanPart::Items);
    }
    if before.goal != after.goal {
        what.push(PlanPart::Goal);
    }
    if before.steps != after.steps {
        what.push(PlanPart::Steps);
    }
    if before.text != after.text {
        what.push(PlanPart::Text);
    }
    what
}
