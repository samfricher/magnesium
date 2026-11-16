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

#[test]
fn the_checks_alone_take_three_and_end_with_the_last_answer_or_decline() {
    let (root, workspace) = fixture();
    due(&workspace, &["skill 1", "skill 2", "skill 3", "skill 4"]);
    let mut s = Sittings::open(&workspace).unwrap();
    let start = now_ms();
    assert_eq!(
        s.ask_checks_only(&workspace, Some("claude:a"), start)
            .unwrap(),
        3
    );
    assert!(s.checks_only().unwrap().is_some());
    assert_eq!(s.checks_only_left().unwrap(), Some(3));

    // The sitting's opener is the checks alone: no paper redos, no new material.
    let mut report = GapLogReport::default();
    let opener = s
        .take_opener(&workspace, start, &mut report)
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

    // Answers and a decline settle the checks; the sitting ends with the last.
    s.settle(&[("q-0".into(), "passed"), ("q-1".into(), "failed")])
        .unwrap();
    assert!(
        !s.end_checks_only_when_done(Some("claude:a"), start + 60_000)
            .unwrap()
    );
    assert_eq!(s.checks_only_left().unwrap(), Some(1));
    s.settle(&[("q-2".into(), "declined")]).unwrap();
    assert!(
        s.end_checks_only_when_done(Some("claude:a"), start + 120_000)
            .unwrap()
    );
    let state = s.now_state().unwrap().unwrap();
    assert_eq!(state.ended_ms, Some(start + 120_000));
    assert_eq!(state.ended_by.as_deref(), Some("claude:a"));
    // Ended, the sitting is an ordinary one again: a learner who asks for
    // more is taught as in any sitting.
    assert!(s.checks_only().unwrap().is_none());
    assert!(
        !s.end_checks_only_when_done(Some("claude:a"), start + 180_000)
            .unwrap()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn asked_again_in_an_open_sitting_it_poses_a_declined_check_and_the_next_due() {
    let (root, workspace) = fixture();
    due(
        &workspace,
        &["skill 1", "skill 2", "skill 3", "skill 4", "skill 5"],
    );
    let mut s = Sittings::open(&workspace).unwrap();
    let start = now_ms();
    // A sitting begun by an ordinary message takes three checks.
    assert!(s.note_learner_message(&workspace, start).unwrap());
    s.settle(&[("q-0".into(), "passed"), ("q-1".into(), "declined")])
        .unwrap();
    // An hour later, the same sitting: the learner asks for the checks alone.
    assert_eq!(
        s.ask_checks_only(&workspace, None, start + HOUR).unwrap(),
        3
    );
    let opener = s.checks_only_opener(&workspace).unwrap().unwrap();
    // Check 3 still open, the declined skill 2 again under a new number, and
    // the next due, skill 4. The passed skill 1 is not posed again.
    assert_eq!(
        numbers(&opener),
        [
            (3, "skill 3".into()),
            (4, "skill 2".into()),
            (5, "skill 4".into())
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
    assert_eq!(s.ask_checks_only(&workspace, None, now_ms()).unwrap(), 0);
    assert!(s.checks_only().unwrap().is_none());
    assert!(s.checks_only_opener(&workspace).unwrap().is_none());
    std::fs::remove_dir_all(root).unwrap();
}
