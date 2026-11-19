use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gaplog::{self, Table};
use crate::runtime::{self, Step};
use crate::study;

const MAX_PLAN_BYTES: u64 = 256 * 1024;
const MAX_ITEMS: usize = 60;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanItem {
    pub id: String,
    pub title: String,
    pub status: String,
    pub day: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub target: Option<String>,
    pub title: Option<String>,
    pub exam_date: Option<String>,
    pub goal: Option<String>,
    pub steps: Vec<Step>,
    pub items: Vec<PlanItem>,
    pub text: Option<String>,
    pub path: Option<String>,
    pub changed_ms: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanPart {
    ExamDate,
    Items,
    Goal,
    Steps,
    Text,
}

fn text(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

pub fn goal_steps(root: &Path) -> Vec<Step> {
    runtime::lesson(root, None)
        .flatten()
        .map(|lesson| lesson.steps.into_iter().take(12).collect())
        .unwrap_or_default()
}

pub fn read(folder: &Path) -> Plan {
    let Ok(digest) = runtime::digest(folder) else {
        return Plan::default();
    };
    if digest["active"].as_bool() != Some(true) || digest["upgrade_needed"].as_bool() == Some(true)
    {
        return Plan::default();
    }
    let root = study::root_of(&digest, folder);
    let mut plan = Plan {
        goal: text(&digest["record"]["sessions"][0]["goal"]),
        steps: goal_steps(&root),
        ..Plan::default()
    };
    let plans = digest["plans"].as_array().cloned().unwrap_or_default();
    if let Some(chosen) = study::nearest_plan(&plans) {
        plan.target = text(&chosen["target"]);
        plan.title = text(&chosen["title"]).or_else(|| plan.target.clone());
        plan.exam_date = text(&chosen["date"]);
        plan.path = text(&chosen["path"]);
    }
    if let Some(relative) = &plan.path {
        let file = root.join(relative);
        let small = std::fs::symlink_metadata(&file)
            .is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_PLAN_BYTES);
        if small {
            if let Ok(markdown) = std::fs::read_to_string(&file) {
                plan.items = items(&markdown);
                plan.text = Some(markdown);
            }
            plan.changed_ms = std::fs::metadata(&file)
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|since| i64::try_from(since.as_millis()).ok());
        }
    }
    plan
}

fn slug(title: &str) -> String {
    let mut slug = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() {
        "item".to_owned()
    } else {
        slug
    }
}

fn status_of(cell: &str) -> &'static str {
    let cell = cell.to_lowercase();
    if ["done", "✓", "✔", "[x]", "finished", "complete"]
        .iter()
        .any(|word| cell.contains(word))
    {
        "done"
    } else if ["doing", "now", "in progress", "current", "started"]
        .iter()
        .any(|word| cell.contains(word))
    {
        "doing"
    } else {
        "todo"
    }
}

pub fn items(markdown: &str) -> Vec<PlanItem> {
    let table: Table = gaplog::table(gaplog::section(markdown, "Blocks"));
    let mut seen: Vec<String> = Vec::new();
    table
        .rows
        .iter()
        .filter_map(|row| {
            let title = table.cell(row, &["Block"]);
            if title.is_empty() {
                return None;
            }
            let base = slug(title);
            let mut id = base.clone();
            let mut n = 1;
            while seen.contains(&id) {
                n += 1;
                id = format!("{base}-{n}");
            }
            seen.push(id.clone());
            let day = table.cell(row, &["Day", "Date", "When"]);
            let note = table.cell(row, &["Purpose", "Note", "Notes"]);
            Some(PlanItem {
                id,
                title: title.to_owned(),
                status: status_of(table.cell(row, &["Status", "Done", "State"])).to_owned(),
                day: gaplog::first_date(day).map(|date| date.to_string()),
                note: (!note.is_empty()).then(|| note.to_owned()),
            })
        })
        .take(MAX_ITEMS)
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Midterm 2: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 2 |\n| When | 2026-11-22 (Sun) 10:00 |\n\n## 6. Blocks\n\n<!-- Hours, not clock times. -->\n\n| # | Block | Purpose | Status | Day |\n|---|---|---|---|---|\n| 1 | Sampling distributions | the standard error | done | 2026-11-12 |\n| 2 | Confidence intervals | t intervals for a mean | doing | 2026-11-14 |\n| 3 | Hypothesis tests | | | |\n| 4 | <name> | <purpose> | | |\n\n## 7. Sessions\n\n- none planned\n";

    #[test]
    fn the_blocks_table_is_the_plans_list() {
        let items = items(PLAN);
        assert_eq!(items.len(), 3, "a placeholder row is no item");
        assert_eq!(items[0].id, "sampling-distributions");
        assert_eq!(items[0].status, "done");
        assert_eq!(items[0].day.as_deref(), Some("2026-11-12"));
        assert_eq!(items[1].status, "doing");
        assert_eq!(items[1].note.as_deref(), Some("t intervals for a mean"));
        assert_eq!(items[2].status, "todo");
        assert_eq!(items[2].note, None);
        assert_eq!(items[2].day, None);
    }

    #[test]
    fn an_item_keeps_its_id_when_the_plan_moves_it() {
        let moved = PLAN.replace(
            "| 2 | Confidence intervals | t intervals for a mean | doing | 2026-11-14 |\n| 3 | Hypothesis tests | | | |",
            "| 2 | Hypothesis tests | | | |\n| 3 | Confidence intervals | t intervals for a mean | doing | 2026-11-14 |",
        );
        let ids = |text: &str| {
            items(text)
                .into_iter()
                .map(|item| item.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(&moved),
            vec![
                "sampling-distributions",
                "hypothesis-tests",
                "confidence-intervals"
            ]
        );
        let before = Plan {
            items: items(PLAN),
            text: Some(PLAN.into()),
            ..Plan::default()
        };
        let after = Plan {
            items: items(&moved),
            text: Some(moved),
            ..Plan::default()
        };
        assert_eq!(
            changes(&before, &after),
            vec![PlanPart::Items, PlanPart::Text]
        );
        assert!(changes(&after, &after).is_empty());
    }

    #[test]
    fn a_plan_with_no_blocks_lists_nothing() {
        assert!(items(
            "# A topic\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Topic |\n"
        )
        .is_empty());
        assert_eq!(slug("Paper B (timed)"), "paper-b-timed");
        assert_eq!(slug("—"), "item");
        let twice = "## Blocks\n\n| Block |\n|---|\n| Redo |\n| Redo |\n";
        let ids: Vec<String> = items(twice).into_iter().map(|item| item.id).collect();
        assert_eq!(ids, vec!["redo", "redo-2"]);
    }
}
