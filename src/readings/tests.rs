use super::*;
use crate::learner::practice::{ItemFacts, ItemState, Origin};
use chrono::NaiveDate;

fn row(plan: &str, n: i64, tag: &str, passes: i64, next_on: Option<NaiveDate>) -> Item {
    let day = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    Item {
        facts: ItemFacts {
            item_id: format!("g-{n}"),
            origin: Origin::Paper,
            label: tag.into(),
            plan: Some(plan.into()),
            row_no: Some(n),
            paper: Some("MockA".into()),
            paper_question: Some("1".into()),
            logged_on: day,
            gap_type: None,
            what_went_wrong: String::new(),
            repair: String::new(),
            confidence: None,
            next_question: None,
            answer_key: None,
            dropped: None,
            base_passes: 0,
            base_next_on: None,
            base_last_pass_on: None,
            base_history: String::new(),
            base_misses: Vec::new(),
            created_at_ms: 0,
            events: Vec::new(),
        },
        state: ItemState {
            passes,
            next_on,
            history: Vec::new(),
            confidence: None,
            misses: Vec::new(),
            effects: Vec::new(),
            passes_after: Vec::new(),
        },
    }
}

fn attempt(tag: &str, right: bool, cold: bool, slow: bool) -> TagAttempt {
    TagAttempt {
        label_key: tag.into(),
        right,
        partly: false,
        alone: true,
        cold,
        slow,
        place: "MockA Q2".into(),
    }
}

fn scope() -> BTreeSet<String> {
    ["chain-rule", "power-rule", "algebra"]
        .map(String::from)
        .into()
}

fn readings(items: &[Item], attempts: &[TagAttempt]) -> BTreeMap<String, (Reading, String)> {
    read_plan(&PlanInputs {
        plan: "midterm-1",
        scoped: &scope(),
        items,
        attempts,
        imported: &BTreeMap::new(),
    })
    .into_iter()
    .map(|r| (r.tag, (r.reading, r.why)))
    .collect()
}

#[test]
fn with_no_evidence_nothing_is_read() {
    assert!(readings(&[], &[]).is_empty());
}

#[test]
fn a_cold_pass_is_sound_for_its_own_item_and_passes_nothing_on() {
    let read = readings(&[], &[attempt("chain-rule", true, true, false)]);
    assert_eq!(
        read["chain-rule"],
        (Reading::Sound, "cold in MockA Q2".into())
    );
    assert!(!read.contains_key("power-rule"));
    assert!(!read.contains_key("algebra"));
}

#[test]
fn an_answer_on_a_label_outside_the_scope_and_with_no_row_is_not_read_for_the_plan() {
    let read = readings(&[], &[attempt("limits", true, true, false)]);
    assert!(read.is_empty());
}

#[test]
fn a_slow_or_helped_pass_is_conditional() {
    let read = readings(&[], &[attempt("chain-rule", true, true, true)]);
    assert_eq!(read["chain-rule"].0, Reading::Conditional);
    assert!(read["chain-rule"].1.contains("slow"));
    let read = readings(&[], &[attempt("chain-rule", true, false, false)]);
    assert_eq!(read["chain-rule"].0, Reading::Conditional);
}

#[test]
fn a_reading_follows_its_gap_row_and_an_open_row_holds_nothing_else_back() {
    let day = NaiveDate::from_ymd_opt(2026, 11, 12);
    let passed = [attempt("chain-rule", true, true, false)];
    let read = readings(&[row("midterm-1", 1, "power-rule", 0, None)], &passed);
    assert_eq!(
        read["power-rule"],
        (
            Reading::Open,
            "midterm-1#1 open, waiting for its repair".into()
        )
    );
    assert_eq!(read["chain-rule"].0, Reading::Sound);
    let read = readings(&[row("midterm-1", 1, "power-rule", 1, day)], &passed);
    assert_eq!(
        read["power-rule"],
        (Reading::Conditional, "midterm-1#1 at 1 of 3".into())
    );
    let read = readings(&[row("midterm-1", 1, "power-rule", 3, None)], &[]);
    assert_eq!(
        read["power-rule"],
        (Reading::Sound, "midterm-1#1 closed".into())
    );
    let read = readings(&[row("final", 1, "power-rule", 0, None)], &[]);
    assert!(!read.contains_key("power-rule"));
}

#[test]
fn a_row_on_any_label_is_read() {
    let read = readings(&[row("midterm-1", 1, "limits", 0, None)], &[]);
    assert_eq!(read["limits"].0, Reading::Open);
}

#[test]
fn a_reading_brought_in_from_a_memory_counts_as_evidence() {
    let imported = BTreeMap::from([(
        "algebra".to_owned(),
        ("sound".to_owned(), "sound from Diagnostic Q1".to_owned()),
    )]);
    let read = read_plan(&PlanInputs {
        plan: "midterm-1",
        scoped: &scope(),
        items: &[],
        attempts: &[],
        imported: &imported,
    });
    let algebra = read.iter().find(|r| r.tag == "algebra").unwrap();
    assert_eq!(algebra.reading, Reading::Sound);
    assert!(algebra.why.contains("Diagnostic Q1"));
}
