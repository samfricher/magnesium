use super::*;

#[test]
fn the_opener_asks_for_question_cold_and_next_and_long_text_never_cuts_that_off() {
    let long = format!("{}...", "é".repeat(MAX_MISS_CHARS));
    let checks: Vec<Check> = (1..=MAX_CHECKS_PER_SITTING as i64)
        .map(|number| Check {
            number,
            label: "x".repeat(120),
            misses: vec![long.clone(); 3],
            question: Some("q".repeat(900)),
            key: Some("k".repeat(900)),
        })
        .collect();
    let redos = vec![DueRedos {
        gap_log: "Study/midterm-1/gap-log.md".into(),
        due: 3,
        first: Some("Diagnostic Q4".into()),
        rows: (1..=3)
            .map(|n| crate::practice::DueRow {
                name: format!("midterm-1#{n}"),
                tag: "km".into(),
                question: Some("q".repeat(900)),
                key: Some("k".repeat(900)),
            })
            .collect(),
    }];
    // Untrimmed, this is over the limit, so the stored text must give way.
    assert!(opener_text_with(&checks, &redos).len() > MAX_OPENER_BYTES);
    let text = opener_text(&checks, &redos);
    assert!(text.len() <= MAX_OPENER_BYTES, "{}", text.len());
    assert!(text.contains(
        "with attempt {label: exactly as above, question: the check as you posed it, first_try, help, cold, next: {question, answer_key}}"
    ));
    assert!(text.contains("cold is true when they answered with no help"));
    assert!(text.contains("first_try declined and cold false."));
    assert!(text.contains("show none of its key before they answer"));
    assert!(text.ends_with("which picks and records them."), "{text}");
    assert!(text.contains("midterm-1#1, tag \"km\": stored question: "));
    // A short opener is untouched.
    let short = vec![Check {
        number: 1,
        label: "sign of b".into(),
        misses: vec!["Took b as 7.".into()],
        question: Some("Solve x² − 7x + 10 = 0".into()),
        key: Some("x = 2 or 5".into()),
    }];
    let text = opener_text(&short, &[]);
    assert!(
        text.contains("1. Label \"sign of b\". Earlier misses: Took b as 7. Stored question: Solve x² − 7x + 10 = 0 Key, hidden until they answer: x = 2 or 5"),
        "{text}"
    );
}

const HOUR: i64 = 60 * 60 * 1000;

fn fixture() -> (PathBuf, Workspace) {
    let root = std::env::temp_dir().join(crate::learner::store::new_id("sittings-test").unwrap());
    std::fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
    std::fs::write(
        root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
        "schema_version = 1\nworkspace_id = \"sittings-test\"\n",
    )
    .unwrap();
    let workspace = Workspace::discover(&root).unwrap().unwrap();
    crate::learner::LearnerStore::initialize(&workspace.store_path(), "sittings-test").unwrap();
    (root, workspace)
}

/// Quick checks due today, as the learner store holds them.
fn due_checks(workspace: &Workspace, labels: &[&str]) {
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

#[test]
fn a_sitting_begins_after_three_hours_and_takes_at_most_three_checks() {
    let (root, workspace) = fixture();
    due_checks(
        &workspace,
        &["skill 1", "skill 2", "skill 3", "skill 4", "skill 5"],
    );
    let mut s = Sittings::open(&workspace).unwrap();
    let start = now_ms();
    assert!(s.note_learner_message(&workspace, start).unwrap());
    assert!(
        !s.note_learner_message(&workspace, start + 2 * HOUR)
            .unwrap()
    );
    assert!(
        !s.note_learner_message(&workspace, start + 4 * HOUR + HOUR / 2)
            .unwrap(),
        "2.5 hours after the last message"
    );
    assert_eq!(s.open_counts().unwrap(), Some(Counts { due: 3, posed: 0 }));
    let not_taken: i64 = s
        .conn
        .query_row(
            "SELECT not_taken FROM sittings ORDER BY sitting_no DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(not_taken, 2, "five due, three taken");
    // "Quick check 1" is not "Quick check 12"; a decline settles a check.
    s.note_reply("Quick check 12 is not here.").unwrap();
    assert_eq!(s.open_counts().unwrap(), Some(Counts { due: 3, posed: 0 }));
    s.note_reply("QUICK CHECK 2: try this.").unwrap();
    s.settle(&[("q-0".into(), "declined")]).unwrap();
    assert_eq!(s.open_counts().unwrap(), Some(Counts { due: 3, posed: 2 }));
    let mut report = GapLogReport::default();
    let opener = s
        .take_opener(&workspace, start, &mut report)
        .unwrap()
        .unwrap();
    assert!(opener.contains("1. Label \"skill 1\"."), "{opener}");
    assert_eq!(
        s.take_opener(&workspace, start, &mut report).unwrap(),
        None,
        "once per sitting"
    );
    let log = std::fs::read_to_string(root.join(".magnesium").join(LOG_FILE)).unwrap();
    let line: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
    assert_eq!(line["declined"], json!(["skill 1"]));
    assert_eq!(line["due_not_taken"], 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_version_1_store_moves_its_checks_to_the_learner_store_once() {
    let (root, workspace) = fixture();
    let path = workspace.control().join(STORE_FILE);
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta(singleton INTEGER PRIMARY KEY CHECK (singleton = 1), version INTEGER NOT NULL, last_learner_ms INTEGER);
             INSERT INTO meta VALUES(1,1,1000);
             CREATE TABLE items(item_id TEXT PRIMARY KEY, label TEXT NOT NULL, label_key TEXT NOT NULL UNIQUE,
               status TEXT NOT NULL CHECK (status IN ('open','closed')), passes INTEGER NOT NULL DEFAULT 0,
               due_day TEXT, last_pass_day TEXT, misses_json TEXT NOT NULL DEFAULT '[]', created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL);
             CREATE TABLE attempts(observation_id TEXT PRIMARY KEY, label TEXT NOT NULL, label_key TEXT NOT NULL, first_try TEXT NOT NULL,
               help TEXT NOT NULL, day TEXT NOT NULL, sitting_id TEXT, effect TEXT NOT NULL, created_ms INTEGER NOT NULL);
             CREATE TABLE sittings(sitting_no INTEGER PRIMARY KEY, sitting_id TEXT NOT NULL UNIQUE, started_ms INTEGER NOT NULL, day TEXT NOT NULL,
               opener_delivered INTEGER NOT NULL DEFAULT 0, redos_json TEXT NOT NULL DEFAULT '[]', not_taken INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE sitting_items(sitting_id TEXT NOT NULL REFERENCES sittings(sitting_id), item_id TEXT NOT NULL REFERENCES items(item_id),
               number INTEGER NOT NULL, label TEXT NOT NULL, posed INTEGER NOT NULL DEFAULT 0,
               outcome TEXT CHECK (outcome IN ('passed','failed','helped','declined')), PRIMARY KEY (sitting_id, item_id));
             INSERT INTO items VALUES('check-1','sign of b','sign of b','open',1,'2026-11-05','2026-11-02','[\"Took b as 7.\"]',1000,2000);
             INSERT INTO items VALUES('check-2','units','units','closed',3,NULL,'2026-11-01','[]',1000,2000);
             INSERT INTO sittings VALUES(1,'sitting-1',1000,'2026-11-02',1,'[]',0);
             INSERT INTO sitting_items VALUES('sitting-1','check-1',1,'sign of b',1,'passed');",
        )
        .unwrap();
    }
    let s = Sittings::open_existing(&workspace).unwrap().unwrap();
    let version: i64 = s
        .conn
        .query_row("SELECT version FROM meta", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 2);
    let tables: i64 = s
        .conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name IN ('items','attempts')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tables, 0);
    let kept: (String, String) = s
        .conn
        .query_row("SELECT item_id,outcome FROM sitting_items", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(kept, ("check-1".into(), "passed".into()));
    let store = crate::practice::open_store(&workspace).unwrap();
    let items = crate::practice::snapshot(&store.conn, workspace.root()).unwrap();
    let state = |id: &str| {
        let item = items.iter().find(|i| i.facts.item_id == id).unwrap();
        (
            item.state.status(),
            item.state.next_on.map(|d| d.to_string()),
            item.state.misses.clone(),
        )
    };
    assert_eq!(
        state("check-1"),
        (
            "1 of 3",
            Some("2026-11-05".into()),
            vec!["Took b as 7.".to_owned()]
        )
    );
    assert_eq!(state("check-2"), ("closed", None, vec![]));
    std::fs::remove_dir_all(root).unwrap();
}
