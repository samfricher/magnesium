use std::path::PathBuf;

use rusqlite::params;

use super::*;
use crate::practice::GapLogReport;
use crate::sittings::now_ms;

const HOUR: i64 = 60 * 60 * 1000;

fn fixture() -> (PathBuf, Workspace) {
    let root =
        std::env::temp_dir().join(crate::learner::store::new_id("checks-only-test").unwrap());
    std::fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
    std::fs::write(
        root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
        "schema_version = 1\nworkspace_id = \"checks-only-test\"\n",
    )
    .unwrap();
    let workspace = Workspace::discover(&root).unwrap().unwrap();
    crate::learner::LearnerStore::initialize(&workspace.store_path(), "checks-only-test").unwrap();
    (root, workspace)
}

/// Quick checks due today, longest due first, as the learner store holds them.
fn due(workspace: &Workspace, labels: &[&str]) {
    let store = crate::practice::open_store(workspace).unwrap();
    for (n, label) in labels.iter().enumerate() {
        store
            .conn
            .execute(
                "INSERT INTO practice_items(item_id,origin,label,label_key,logged_on,base_next_on,created_at_ms,updated_at_ms)
                 VALUES(?1,'chat',?2,?3,'2026-01-01','2026-01-02',?4,?4)",
                params![format!("q-{n}"), label, crate::learner::label_key(label), n as i64],
            )
            .unwrap();
    }
}

fn numbers(opener: &Opener) -> Vec<(i64, String)> {
    opener
        .checks
        .as_array()
        .unwrap()
        .iter()
        .map(|check| {
            (
                check["number"].as_i64().unwrap(),
                check["label"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

const A: Option<&str> = Some("claude:a");
const B: Option<&str> = Some("claude:b");

#[test]
fn the_checks_alone_take_three_and_end_with_the_last_answer_or_decline() {
    let (root, workspace) = fixture();
    due(&workspace, &["skill 1", "skill 2", "skill 3", "skill 4"]);
    let mut s = Sittings::open(&workspace).unwrap();
    let start = now_ms();
    assert_eq!(s.ask_checks_only(&workspace, A, start).unwrap(), 3);
    assert!(s.checks_only(A).unwrap().is_some());
    assert_eq!(s.checks_only_left(&workspace, A, start).unwrap(), Some(3));

    // The opener is the checks alone: no paper redos, no new material, once.
    let opener = s
        .take_checks_only_opener(&workspace, A, start)
        .unwrap()
        .unwrap();
    assert_eq!(
        numbers(&opener),
        [
            (1, "skill 1".into()),
            (2, "skill 2".into()),
            (3, "skill 3".into())
        ]
    );
    assert_eq!(opener.redos, json!([]));
    assert!(opener.text.starts_with(LEAD), "{}", opener.text);
    assert!(opener.text.ends_with(CLOSE), "{}", opener.text);
    assert!(
        opener
            .text
            .contains("Your next reply begins with the words \"Quick check 1:\""),
        "{}",
        opener.text
    );
    assert!(!opener.text.contains("Paper redos"), "{}", opener.text);
    assert!(
        s.take_checks_only_opener(&workspace, A, start)
            .unwrap()
            .is_none()
    );
    // It claimed the sitting's usual opener too.
    let mut report = GapLogReport::default();
    assert!(
        s.take_opener(&workspace, start, &mut report)
            .unwrap()
            .is_none()
    );

    // Answers and a decline settle the checks; the sitting ends with the last.
    s.settle(&[("q-0".into(), "passed"), ("q-1".into(), "failed")])
        .unwrap();
    assert!(
        !s.end_checks_only_when_done(&workspace, A, start + 60_000)
            .unwrap()
    );
    assert_eq!(s.checks_only_left(&workspace, A, start).unwrap(), Some(1));
    s.settle(&[("q-2".into(), "declined")]).unwrap();
    assert!(
        s.end_checks_only_when_done(&workspace, A, start + 120_000)
            .unwrap()
    );
    let state = s.now_state().unwrap().unwrap();
    assert_eq!(state.ended_ms, Some(start + 120_000));
    assert_eq!(state.ended_by.as_deref(), Some("claude:a"));
    // Ended, the sitting is an ordinary one again, and stays one when the
    // learner writes again.
    assert!(s.checks_only(A).unwrap().is_none());
    s.reopen().unwrap();
    assert!(s.checks_only(A).unwrap().is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_checks_alone_belong_to_the_conversation_that_asked() {
    let (root, workspace) = fixture();
    due(&workspace, &["skill 1", "skill 2"]);
    let mut s = Sittings::open(&workspace).unwrap();
    let start = now_ms();
    // Conversation B began the sitting and had its usual opener.
    assert!(s.note_learner_message(&workspace, start).unwrap());
    let mut report = GapLogReport::default();
    assert!(
        s.take_opener(&workspace, start, &mut report)
            .unwrap()
            .is_some()
    );
    // Then A asks for the checks alone.
    assert_eq!(s.ask_checks_only(&workspace, A, start + 1).unwrap(), 2);
    // B goes on as in any sitting: nothing of A's reaches it.
    assert!(s.checks_only(B).unwrap().is_none());
    assert_eq!(s.checks_only_left(&workspace, B, start).unwrap(), None);
    assert!(
        s.take_checks_only_opener(&workspace, B, start)
            .unwrap()
            .is_none()
    );
    assert!(!s.lift_checks_only(B).unwrap());
    assert!(!s.end_checks_only_when_done(&workspace, B, start).unwrap());
    // A gets its checks although the usual opener went to B.
    assert!(
        s.take_checks_only_opener(&workspace, A, start)
            .unwrap()
            .is_some()
    );
    // A asks for something else: an ordinary sitting, its checks still due.
    assert!(s.lift_checks_only(A).unwrap());
    assert!(s.checks_only(A).unwrap().is_none());
    assert!(s.now_state().unwrap().unwrap().ended_ms.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_check_the_record_no_longer_has_due_is_not_waited_for() {
    let (root, workspace) = fixture();
    due(&workspace, &["skill 1", "skill 2"]);
    let mut s = Sittings::open(&workspace).unwrap();
    let start = now_ms();
    assert_eq!(s.ask_checks_only(&workspace, A, start).unwrap(), 2);
    s.settle(&[("q-0".into(), "passed")]).unwrap();
    assert!(!s.end_checks_only_when_done(&workspace, A, start).unwrap());
    // The second check's answer went under another name, and the record now
    // has it due on a later day: the sitting does not wait for it.
    crate::practice::open_store(&workspace)
        .unwrap()
        .conn
        .execute(
            "UPDATE practice_items SET base_next_on='2999-01-01' WHERE item_id='q-1'",
            [],
        )
        .unwrap();
    assert_eq!(s.checks_only_left(&workspace, A, start).unwrap(), Some(0));
    assert!(s.end_checks_only_when_done(&workspace, A, start).unwrap());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn asked_in_an_open_sitting_it_keeps_its_checks_and_takes_the_next_due() {
    let (root, workspace) = fixture();
    due(
        &workspace,
        &["skill 1", "skill 2", "skill 3", "skill 4", "skill 5"],
    );
    let mut s = Sittings::open(&workspace).unwrap();
    let start = now_ms();
    // A sitting begun by an ordinary message takes three checks, and two are
    // settled in it.
    assert!(s.note_learner_message(&workspace, start).unwrap());
    s.settle(&[("q-0".into(), "passed"), ("q-1".into(), "declined")])
        .unwrap();
    // An hour later, the same sitting: the learner asks for the checks alone.
    assert_eq!(
        s.ask_checks_only(&workspace, None, start + HOUR).unwrap(),
        3
    );
    let opener = s
        .checks_only_opener(&workspace, None, start + HOUR)
        .unwrap()
        .unwrap();
    // Check 3 still open, then the next due under new numbers. A check the
    // sitting already holds is never taken twice.
    assert_eq!(
        numbers(&opener),
        [
            (3, "skill 3".into()),
            (4, "skill 4".into()),
            (5, "skill 5".into())
        ]
    );
    assert!(
        opener.text.contains("\"Quick check 3:\""),
        "{}",
        opener.text
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn with_nothing_due_the_sitting_stays_as_it_was() {
    let (root, workspace) = fixture();
    let mut s = Sittings::open(&workspace).unwrap();
    let now = now_ms();
    assert_eq!(s.ask_checks_only(&workspace, None, now).unwrap(), 0);
    assert!(s.checks_only(None).unwrap().is_none());
    assert!(
        s.checks_only_opener(&workspace, None, now)
            .unwrap()
            .is_none()
    );
    std::fs::remove_dir_all(root).unwrap();
}
