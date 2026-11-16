use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDate;
use serde_json::json;

use super::{Answer, ItemFacts, Miss, Receipt, receipts, recorded_answers, terminal_lines};

fn day(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn answer(id: &str, label: &str, mark: &str, on: &str) -> Answer {
    Answer {
        id: id.into(),
        label: label.into(),
        mark: mark.into(),
        paper: None,
        day: day(on),
    }
}

fn wanted(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn of(answers: &[Answer], id: &str) -> Option<Receipt> {
    receipts(answers, &BTreeMap::new(), &BTreeMap::new(), &wanted(&[id])).remove(id)
}

#[test]
fn a_pass_after_a_miss_names_the_miss_and_the_days_since_the_last_answer() {
    let answers = [
        answer("a1", "sign of b", "wrong", "2026-11-10"),
        answer("a2", "sign of b", "right", "2026-11-14"),
    ];
    let receipt = of(&answers, "a2").unwrap();
    assert_eq!(
        receipt.miss,
        Some(Miss {
            day: day("2026-11-10"),
            label: "sign of b".into(),
            mark: "wrong".into(),
            paper: None,
        })
    );
    assert_eq!(receipt.days, Some(4));
    assert_eq!(
        receipt.text(),
        "Not right on Tue 10 Nov: sign of b. It held through the 4 days since you last answered it."
    );
}

#[test]
fn the_days_count_from_the_last_answer_and_the_miss_is_the_latest_before_it() {
    let answers = [
        answer("a1", "sign of b", "wrong", "2026-11-03"),
        answer("a2", "sign of b", "partly", "2026-11-05"),
        answer("a3", "sign of b", "right", "2026-11-06"),
        answer("other", "units", "wrong", "2026-11-08"),
        answer("a4", "sign of b", "right", "2026-11-09"),
    ];
    let receipt = of(&answers, "a4").unwrap();
    assert_eq!(receipt.miss.as_ref().unwrap().day, day("2026-11-05"));
    assert_eq!(receipt.previous, Some(day("2026-11-06")));
    assert_eq!(
        receipt.text(),
        "Partly right on Thu 5 Nov: sign of b. It held through the 3 days since you last answered it."
    );
}

#[test]
fn a_skill_never_missed_says_only_the_days_and_a_first_answer_gets_none() {
    let answers = [
        answer("a1", "units", "right", "2026-11-02"),
        answer("a2", "units", "right", "2026-11-03"),
    ];
    assert_eq!(of(&answers, "a1"), None);
    assert_eq!(
        of(&answers, "a2").unwrap().text(),
        "It held through the day since you last answered it."
    );
}

#[test]
fn a_declined_question_is_not_an_answer() {
    let answers = [
        answer("a1", "units", "right", "2026-11-02"),
        answer("a2", "units", "declined", "2026-11-04"),
        answer("a3", "units", "right", "2026-11-05"),
    ];
    assert_eq!(of(&answers, "a3").unwrap().days, Some(3));
}

#[test]
fn an_answer_under_the_graphs_name_finds_the_earlier_one_under_the_tutors_spelling() {
    let answers = [
        answer("a1", "Chain rule", "wrong", "2026-11-02"),
        answer("a2", "chain-rule", "right", "2026-11-04"),
    ];
    let receipt = of(&answers, "a2").unwrap();
    assert_eq!(receipt.miss.unwrap().label, "Chain rule");
    assert_eq!(receipt.days, Some(2));
}

#[test]
fn a_redo_finds_the_paper_answer_that_made_its_row_whatever_its_label() {
    let mut lost = answer("a1", "slope and Km", "partly", "2026-11-02");
    lost.paper = Some("Midterm 2023".into());
    let answers = [
        lost,
        answer("a2", "reading Km from a plot", "right", "2026-11-05"),
    ];
    let links = BTreeMap::from([
        ("a1".to_owned(), BTreeSet::from(["g1".to_owned()])),
        ("a2".to_owned(), BTreeSet::from(["g1".to_owned()])),
    ]);
    let items = BTreeMap::from([(
        "g1".to_owned(),
        ItemFacts {
            label: "slope and Km".into(),
            paper: Some("Midterm 2023".into()),
            ..ItemFacts::default()
        },
    )]);
    let receipt = receipts(&answers, &links, &items, &wanted(&["a2"]))
        .remove("a2")
        .unwrap();
    assert_eq!(
        receipt.text(),
        "Lost marks on Midterm 2023, Mon 2 Nov: slope and Km. It held through the 3 days since you last answered it."
    );
}

#[test]
fn a_row_brought_in_from_a_gap_log_file_is_the_miss_when_no_answer_made_it() {
    let answers = [answer("a2", "slope and Km", "right", "2026-11-05")];
    let links = BTreeMap::from([("a2".to_owned(), BTreeSet::from(["g1".to_owned()]))]);
    let items = BTreeMap::from([(
        "g1".to_owned(),
        ItemFacts {
            label: "slope and Km".into(),
            paper: Some("Midterm 2023".into()),
            brought_in_on: Some(day("2026-11-01")),
            brought_in_pass_on: None,
        },
    )]);
    let receipt = receipts(&answers, &links, &items, &wanted(&["a2"]))
        .remove("a2")
        .unwrap();
    assert_eq!(receipt.miss.unwrap().mark, "lost");
    assert_eq!(receipt.days, Some(4));
}

#[test]
fn no_word_counts_days_away() {
    // Whatever the gap, the line says what the answer held through.
    for days in [1, 2, 9, 40] {
        let text = Receipt {
            miss: None,
            previous: None,
            days: Some(days),
        }
        .text();
        for word in ["missed", "away", "streak", "since your last visit", "gap"] {
            assert!(!text.to_lowercase().contains(word), "{text}");
        }
    }
}

#[test]
fn a_terminal_line_says_which_pass_and_the_receipt() {
    let progress = json!({
        "plans": [],
        "other": {"skills": [
            {"skill": "sign of b", "passed_by": ["a1", "a2"]},
            {"skill": "units", "passed_by": ["u1"]},
        ]},
        "pass_receipts": {
            "a2": {"missed": "sign of b", "text": "Not right on Tue 10 Nov: sign of b. It held through the 4 days since you last answered it."},
            "a1": {"missed": null, "text": "It held through the day since you last answered it."},
        },
    });
    assert_eq!(
        terminal_lines(&progress, &wanted(&["a2"])).as_deref(),
        Some(
            "Second of three days. Not right on Tue 10 Nov: sign of b. It held through the 4 days since you last answered it."
        )
    );
    // With no miss to name the skill, the line names it.
    assert_eq!(
        terminal_lines(&progress, &wanted(&["a1"])).as_deref(),
        Some(
            "First of three days on sign of b. It held through the day since you last answered it."
        )
    );
    // A pass with no receipt, and an answer that earned none, say nothing.
    assert_eq!(terminal_lines(&progress, &wanted(&["u1", "x"])), None);
}

#[test]
fn the_recorded_answers_are_read_from_the_tool_result_as_claude_code_hands_it() {
    let result = json!({"update_status": "committed", "practice": {"attempts": [
        {"attempt_id": "a7", "observation": "work", "items": []},
    ]}});
    let handed = json!([{"type": "text", "text": result.to_string()}]);
    assert_eq!(recorded_answers(Some(&handed)), wanted(&["a7"]));
    assert!(recorded_answers(None).is_empty());
}
