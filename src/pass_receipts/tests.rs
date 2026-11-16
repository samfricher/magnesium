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
fn the_skill_is_the_one_the_pass_count_takes_case_and_spaces_aside() {
    // Case and spaces fold, as the record's label key does.
    let answers = [
        answer("a1", "Sign of  b", "wrong", "2026-11-02"),
        answer("a2", "sign of b", "right", "2026-11-04"),
    ];
    let receipt = of(&answers, "a2").unwrap();
    assert_eq!(receipt.miss.unwrap().label, "Sign of  b");
    assert_eq!(receipt.days, Some(2));
}

#[test]
fn two_labels_that_differ_by_a_symbol_are_two_skills() {
    for (missed, passed) in [
        ("integral of e^{-x}", "integral of e^{x}"),
        ("a+b", "a-b"),
        ("x<y", "x>y"),
        ("f'(x)", "f(x)"),
        ("Chain rule", "chain-rule"),
    ] {
        let answers = [
            answer("a1", missed, "wrong", "2026-11-10"),
            answer("a2", passed, "right", "2026-11-14"),
        ];
        assert_eq!(of(&answers, "a2"), None, "{missed} / {passed}");
    }
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
            from_a_paper: true,
            brought_in_on: Some(day("2026-11-01")),
            ..ItemFacts::default()
        },
    )]);
    let receipt = receipts(&answers, &links, &items, &wanted(&["a2"]))
        .remove("a2")
        .unwrap();
    assert_eq!(receipt.miss.unwrap().mark, "lost");
    assert_eq!(receipt.days, Some(4));
}

#[test]
fn a_failed_redo_a_gap_log_file_held_is_the_later_miss() {
    // Logged on 1 Nov, its Passes cell "2026-11-03 pass, 2026-11-06 fail",
    // and a pass in the record on 10 Nov.
    let answers = [answer("a2", "slope and Km", "right", "2026-11-10")];
    let links = BTreeMap::from([("a2".to_owned(), BTreeSet::from(["g1".to_owned()]))]);
    let items = BTreeMap::from([(
        "g1".to_owned(),
        ItemFacts {
            label: "slope and Km".into(),
            paper: Some("Midterm 2023".into()),
            from_a_paper: true,
            brought_in_on: Some(day("2026-11-01")),
            brought_in_pass_on: Some(day("2026-11-03")),
            brought_in_redos: super::redos_in("2026-11-03 pass, 2026-11-06 fail"),
        },
    )]);
    let receipt = receipts(&answers, &links, &items, &wanted(&["a2"]))
        .remove("a2")
        .unwrap();
    let miss = receipt.miss.as_ref().unwrap();
    assert_eq!(
        (miss.day, miss.mark.as_str()),
        (day("2026-11-06"), "failed")
    );
    assert_eq!(receipt.days, Some(4));
    assert_eq!(
        receipt.text(),
        "Not right on Fri 6 Nov: slope and Km. It held through the 4 days since you last answered it."
    );
}

#[test]
fn a_quick_check_an_earlier_version_kept_names_no_miss() {
    let answers = [answer("a2", "units", "right", "2026-11-10")];
    let links = BTreeMap::from([("a2".to_owned(), BTreeSet::from(["q1".to_owned()]))]);
    let items = BTreeMap::from([(
        "q1".to_owned(),
        ItemFacts {
            label: "units".into(),
            brought_in_on: Some(day("2026-11-07")),
            ..ItemFacts::default()
        },
    )]);
    let receipt = receipts(&answers, &links, &items, &wanted(&["a2"]))
        .remove("a2")
        .unwrap();
    assert_eq!((receipt.miss, receipt.days), (None, Some(3)));
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
    // A pass brought in from a gap-log file has no answer: it is counted
    // in which pass the next one is, and says nothing itself.
    let brought_in = json!({
        "plans": [{"plan": "midterm-1", "from": "record", "skills": [
            {"skill": "slope and Km", "passed_by": [null, "r2"]},
        ]}],
        "other": {"skills": []},
        "pass_receipts": {"r2": {"missed": "slope and Km", "text": "Lost marks on Midterm 2023, Mon 2 Nov: slope and Km."}},
    });
    assert_eq!(
        terminal_lines(&brought_in, &wanted(&["r2"])).as_deref(),
        Some("Second of three days. Lost marks on Midterm 2023, Mon 2 Nov: slope and Km.")
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
