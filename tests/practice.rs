//! The runtime works out what is due, and writes the gap log itself (#30).
//! The tutor reports marks, repairs and next questions, never a date, pass
//! or status.
use rusqlite::Connection;
use serde_json::{Value, json};
use support::{TestEnvironment, run_hook_with_host_flag, v2};

const THREAD: &str = "conv";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
const GAP_LOG: &str = "Study/midterm-1/gap-log.md";

fn prompt(e: &TestEnvironment, prompt_id: &str, text: &str) -> String {
    let input = json!({"session_id":THREAD,"cwd":e.workspace,"hook_event_name":"UserPromptSubmit","prompt_id":prompt_id,"prompt":text});
    let notice = v2::context(&run_hook_with_host_flag(
        e,
        &e.workspace,
        "user-prompt-submit",
        input,
        "claude",
    ));
    notice
        .split_once("current event ")
        .unwrap()
        .1
        .split(['.', ' '])
        .next()
        .unwrap()
        .to_owned()
}

fn step(e: &TestEnvironment, request: Value) -> Value {
    let value = v2::tool(e, "claude", THREAD, "learning_step", request);
    assert_eq!(value["update_status"], "committed", "{value}");
    value
}

fn answer(e: &TestEnvironment, id: &str, text: &str, attempt: Value) -> Value {
    let event = prompt(e, id, text);
    step(
        e,
        json!({"schema":"learner-model/v2","action":"update","request_id":id,
            "observations":[{"client_ref":"work","kind":"learner_work","description":"Redo of the slope.",
                "events":[{"event_id":event,"role":"learner_response"}],"attempt":attempt}]}),
    )
}

fn store(e: &TestEnvironment) -> Connection {
    Connection::open(e.workspace.join(".magnesium/state.sqlite")).unwrap()
}

/// Move every recorded time `days` back, as if that many days had passed.
fn days_pass(e: &TestEnvironment, days: i64) {
    let conn = store(e);
    for sql in [
        "UPDATE attempts SET answered_at_ms=answered_at_ms-?1",
        "UPDATE practice_events SET at_ms=at_ms-?1",
    ] {
        conn.execute(sql, [days * DAY_MS]).unwrap();
    }
    let sittings = e.workspace.join(".magnesium/sittings.sqlite");
    if sittings.exists() {
        Connection::open(sittings)
            .unwrap()
            .execute(
                "UPDATE meta SET last_learner_ms=last_learner_ms-?1",
                [days * DAY_MS],
            )
            .unwrap();
    }
}

fn plan(e: &TestEnvironment, gap_log: &str) {
    let dir = e.workspace.join("Study/midterm-1");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plan.md"),
        "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | <YYYY-MM-DD> |\n",
    )
    .unwrap();
    std::fs::write(dir.join("gap-log.md"), gap_log).unwrap();
}

fn gap_log(e: &TestEnvironment) -> String {
    std::fs::read_to_string(e.workspace.join(GAP_LOG)).unwrap()
}

/// The Gaps section, from its heading to the next.
fn gaps(text: &str) -> &str {
    let start = text.find("## Gaps").unwrap();
    let end = text[start + 3..]
        .find("\n## ")
        .map_or(text.len(), |end| start + 3 + end);
    &text[start..end]
}

/// The row line for `#n` in the Gaps table.
fn row(text: &str, n: i64) -> String {
    gaps(text)
        .lines()
        .find(|line| line.starts_with(&format!("| {n} |")))
        .unwrap_or_else(|| panic!("no row {n} in\n{text}"))
        .to_owned()
}

fn digest(e: &TestEnvironment) -> Value {
    let output = e.command().arg("digest").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

const TEMPLATE: &str = include_str!("../claude-plugin/skills/papers/templates/gap-log.md");

#[test]
fn a_row_passed_on_three_allowed_days_closes_with_no_date_written_by_the_tutor() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    plan(&e, TEMPLATE);
    v2::start(&e, "claude", THREAD);

    // A marked paper: one part right, one that lost marks. The tutor gives the
    // gap's type and what went wrong, never a date or a status.
    let event = prompt(&e, "p-1", "Diagnostic: 1. Km = 2 (S) 2. Vmax = 4 (U)");
    let marked = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"paper",
            "observations":[{"client_ref":"paper","kind":"learner_work","description":"Diagnostic, timed.",
                "events":[{"event_id":event,"role":"learner_response"}],
                "attempts":[
                    {"label":"km","question":"Find Km from the plot","first_try":"right","help":"none","cold":true,"confidence":"sure",
                     "paper":{"name":"Diagnostic","question":"1","marks":2,"out_of":2}},
                    {"label":"vmax","question":"Find Vmax from the plot","first_try":"partly","help":"none","cold":true,"confidence":"unsure",
                     "paper":{"name":"Diagnostic","question":"2","marks":1,"out_of":3},
                     "gap":{"type":"K","what_went_wrong":"Read Vmax as the slope; the key needs the y-intercept's reciprocal."}}]}]}),
    );
    let made = &marked["practice"]["items"][0];
    assert_eq!(made["item"], "midterm-1#1", "{marked}");
    assert_eq!(made["status"], "open");
    assert_eq!(made["next_redo"], Value::Null, "no redo before the repair");
    assert!(
        marked["practice"]["todo"][0]
            .as_str()
            .unwrap()
            .contains("repaired"),
        "{marked}"
    );
    assert_eq!(marked["gap_logs"]["written"], json!([GAP_LOG]), "{marked}");
    let text = gap_log(&e);
    assert!(
        row(&text, 1).contains("| Diagnostic | 2 | vmax | Read Vmax as the slope; the key needs the y-intercept's reciprocal. | K | U |  |  |  | open |"),
        "{text}"
    );
    assert!(
        !gaps(&text).contains("<YYYY-MM-DD>"),
        "the template row is gone:\n{text}"
    );
    assert!(
        text.contains("## Scores"),
        "the rest of the file is kept:\n{text}"
    );

    // The repair, and the next question with its key. The first redo falls
    // due the next day, never the same day.
    let repaired = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"repair",
            "practice":[{"item":"midterm-1#1","repaired":"taught reading the intercept",
                "next":{"question":"A Lineweaver–Burk line has intercept 0.25. Find Vmax.","answer_key":"Vmax = 4"}}]}),
    );
    let tomorrow = repaired["practice"]["items"][0]["next_redo"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(repaired["practice"]["items"][0]["status"], "open");
    // A right answer the same day counts neither way.
    let early = answer(
        &e,
        "early",
        "4",
        json!({"label":"vmax","question":"Intercept 0.25: Vmax?","first_try":"right","help":"none","cold":false,"counts_for":["midterm-1#1"]}),
    );
    assert_eq!(
        early["practice"]["attempts"][0]["items"][0]["effect"],
        "no_count"
    );

    // Three cold passes on three allowed days: 1, then 3, then 7 days apart.
    let redo = |id: &str| {
        answer(
            &e,
            id,
            "Vmax = 4",
            json!({"label":"vmax","question":"Intercept 0.25: Vmax?","first_try":"right","help":"none","cold":true,
                "counts_for":["midterm-1#1"],
                "next":{"question":"Intercept 0.5: Vmax?","answer_key":"2"}}),
        )
    };
    days_pass(&e, 1);
    let first = redo("redo-1");
    let state = &first["practice"]["items"][0];
    assert_eq!(state["status"], "1 of 3", "{first}");
    assert_ne!(state["next_redo"].as_str().unwrap(), tomorrow);
    days_pass(&e, 3);
    assert_eq!(redo("redo-2")["practice"]["items"][0]["status"], "2 of 3");
    // Too soon: two days into a seven-day gap.
    days_pass(&e, 2);
    let soon = redo("too-soon");
    assert_eq!(
        soon["practice"]["attempts"][0]["items"][0]["effect"],
        "no_count"
    );
    days_pass(&e, 5);
    let last = redo("redo-3");
    assert_eq!(last["practice"]["items"][0]["status"], "closed", "{last}");
    assert_eq!(last["practice"]["items"][0]["next_redo"], Value::Null);

    let text = gap_log(&e);
    let closed = row(&text, 1);
    assert!(closed.ends_with("|  | closed |"), "{closed}");
    assert_eq!(closed.matches(" pass").count(), 3, "{closed}");
    assert!(closed.contains("taught reading the intercept"), "{closed}");
    let digest = digest(&e);
    assert_eq!(digest["plans"][0]["unfixed_gaps"], 0, "{digest}");
    assert_eq!(digest["plans"][0]["gap_log_from"], "record");
}

#[test]
fn breaking_the_markdown_table_by_hand_changes_nothing_about_what_is_due() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    plan(
        &e,
        "# Gap log: midterm-1\n\n## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n| 1 | 2026-10-20 | Diagnostic | 3 | km | slope read as intercept | K | S | taught the slope | | 2026-10-25 | open |\n\n## Scores\n\n| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n| Diagnostic | 2026-10-20 | 10/20 | 40/45 min | |\n",
    );
    // Loading a paper stage brings the log in and writes its table.
    let load = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    assert_eq!(load["gap_logs"]["imported"], json!([GAP_LOG]), "{load}");
    let before = digest(&e);
    assert_eq!(before["plans"][0]["redos_due"], 1, "{before}");

    // Break the table by hand: drop a cell, rewrite the date, close the row.
    let path = e.workspace.join(GAP_LOG);
    let text = gap_log(&e);
    let broken = text.replace(&row(&text, 1), "| 1 | soon | closed |");
    std::fs::write(&path, &broken).unwrap();
    let after = digest(&e);
    assert_eq!(after["plans"][0]["redos_due"], 1, "{after}");
    assert_eq!(after["plans"][0]["unfixed_gaps"], 1);
    assert_eq!(after["plans"][0]["unreadable"], json!([]), "{after}");

    // The next write notices the edit, saves it, and writes the table again.
    let reload = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers"}),
    );
    let edited = &reload["gap_logs"]["edited"][0];
    assert_eq!(edited["file"], GAP_LOG, "{reload}");
    let saved =
        std::fs::read_to_string(e.workspace.join(edited["saved_to"].as_str().unwrap())).unwrap();
    assert_eq!(saved, broken);
    let restored = gap_log(&e);
    assert!(
        row(&restored, 1).contains("| 2026-10-25 | open |"),
        "{restored}"
    );
    assert!(restored.contains("| Diagnostic | 2026-10-20 | 10/20 | 40/45 min | |"));
}

#[test]
fn a_gap_log_from_an_earlier_template_is_brought_in_with_its_rows_and_dates() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let old = "# Gap log: midterm-1\n\n## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n\
        | 1 | 2026-10-20 | Diagnostic | 3 | km | slope | K | S | taught | 2026-10-21 pass | 2026-10-24 | 1 of 3 |\n\
        | 2 | 2026-10-20 | Diagnostic | 4 | vmax | units | N |  |  |  |  | open |\n\
        | 4 | 2026-10-22 | MockA | 2b | km | slip | A | G | fixed | 2026-10-23 pass, 2026-10-26 pass, 2026-11-02 pass |  | closed |\n";
    plan(&e, old);
    let load = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    assert_eq!(load["gap_logs"]["imported"], json!([GAP_LOG]), "{load}");
    let text = gap_log(&e);
    for (n, ending) in [
        (1, "| S | taught | 2026-10-21 pass | 2026-10-24 | 1 of 3 |"),
        (2, "| N |  |  |  |  | open |"),
        (
            4,
            "| A | G | fixed | 2026-10-23 pass, 2026-10-26 pass, 2026-11-02 pass |  | closed |",
        ),
    ] {
        assert!(row(&text, n).ends_with(ending), "{n}: {text}");
    }
    let backups = std::fs::read_dir(e.workspace.join(".magnesium/backups/gap-logs"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(backups, vec![old.to_owned()]);
    // Row 1 is due; a passing redo on it continues from its second pass.
    let digest = digest(&e);
    assert_eq!(digest["plans"][0]["redos_due"], 1, "{digest}");
    assert_eq!(digest["plans"][0]["unfixed_gaps"], 2);
    v2::start(&e, "claude", THREAD);
    let passed = answer(
        &e,
        "redo",
        "Km = 2",
        json!({"label":"km","question":"Find Km","first_try":"right","help":"none","cold":true,"counts_for":["midterm-1#1"]}),
    );
    assert_eq!(
        passed["practice"]["items"][0]["status"], "2 of 3",
        "{passed}"
    );
    // A second load brings nothing in again.
    let again = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    assert!(again["gap_logs"].get("imported").is_none(), "{again}");
}

#[test]
fn a_paper_answer_that_lost_marks_needs_its_gap_and_names_must_resolve() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    plan(&e, TEMPLATE);
    v2::start(&e, "claude", THREAD);
    let event = prompt(&e, "p-1", "Q2: 4");
    let refused = |attempt: Value| -> Value {
        let output = v2::tool_output(
            &e,
            "claude",
            THREAD,
            "learning_step",
            json!({"schema":"learner-model/v2","action":"update","request_id":"bad",
                "observations":[{"client_ref":"work","kind":"learner_work","description":"Q2.",
                    "events":[{"event_id":event,"role":"learner_response"}],"attempt":attempt}]}),
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value["error"]["recovery"]["field_path"].clone()
    };
    let paper = json!({"name":"Diagnostic","question":"2","marks":1,"out_of":3});
    for (attempt, path) in [
        (
            json!({"label":"vmax","question":"Vmax?","first_try":"partly","help":"none","cold":true,"paper":paper}),
            "observations[0].attempt.gap",
        ),
        (
            json!({"label":"vmax","question":"Vmax?","first_try":"partly","help":"none","cold":true,"paper":paper,
                "counts_for":["midterm-1#9"]}),
            "observations[0].attempt.counts_for[0]",
        ),
        (
            json!({"label":"vmax","question":"Vmax?","first_try":"right","help":"none","cold":true,
                "paper":{"name":"Diagnostic","question":"2","marks":3,"out_of":3},
                "gap":{"type":"K","what_went_wrong":"x"}}),
            "observations[0].attempt.gap.type",
        ),
        (
            json!({"label":"vmax","question":"Vmax?","first_try":"wrong","help":"none","cold":false,
                "gap":{"type":"K","what_went_wrong":"x"}}),
            "observations[0].attempt.gap",
        ),
    ] {
        assert_eq!(refused(attempt), json!(path));
    }
    let output = v2::tool_output(
        &e,
        "claude",
        THREAD,
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"bad-edit",
            "practice":[{"item":"sign of b","repaired":"taught it"}]}),
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["recovery"]["field_path"], "practice[0].item");
    let rows: i64 = store(&e)
        .query_row("SELECT count(*) FROM practice_items", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 0, "nothing is stored from a refused request");
}

#[test]
fn a_corrected_attempt_changes_what_is_due() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    plan(&e, TEMPLATE);
    v2::start(&e, "claude", THREAD);
    let event = prompt(&e, "p-1", "Q2: Vmax = 4");
    step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"paper",
            "observations":[{"client_ref":"paper","kind":"learner_work","description":"Diagnostic.",
                "events":[{"event_id":event,"role":"learner_response"}],
                "attempts":[{"label":"vmax","question":"Vmax?","first_try":"wrong","help":"none","cold":true,
                     "paper":{"name":"Diagnostic","question":"2","marks":0,"out_of":3},
                     "gap":{"type":"K","what_went_wrong":"Read the slope."}}]}]}),
    );
    step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"repair",
            "practice":[{"item":"midterm-1#1","repaired":"taught it"}]}),
    );
    days_pass(&e, 1);
    // A redo marked wrong fails the row, which waits for a new repair.
    let failed = answer(
        &e,
        "redo",
        "Vmax = 2",
        json!({"label":"vmax","question":"Intercept 0.25: Vmax?","first_try":"wrong","help":"none","cold":true,"counts_for":["midterm-1#1"]}),
    );
    assert_eq!(failed["practice"]["items"][0]["next_redo"], Value::Null);
    assert!(row(&gap_log(&e), 1).contains(" fail |"));
    // The learner shows the redo was right: the tutor corrects the attempt,
    // and the row passes instead, with no date from the tutor.
    let attempt = failed["practice"]["attempts"][0]["attempt_id"]
        .as_str()
        .unwrap();
    let corrected = step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"fix",
            "attempt_corrections":[{"attempt_id":attempt,"first_try":"right","reason":"4 was right; I misread it."}]}),
    );
    assert_eq!(
        corrected["practice"]["items"][0]["status"], "1 of 3",
        "{corrected}"
    );
    assert!(row(&gap_log(&e), 1).contains(" pass |"));
    let kept: String = store(&e)
        .query_row(
            "SELECT json_extract(before_json,'$.mark') FROM attempt_corrections",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, "wrong");
    // A row logged in error comes out of the table.
    step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"drop",
            "practice":[{"item":"midterm-1#1","dropped":"the key was wrong, not the learner"}]}),
    );
    assert!(!gaps(&gap_log(&e)).contains("| 1 |"));
}

#[test]
fn a_gap_log_made_after_other_practice_is_still_brought_in_and_one_it_cannot_read_is_never_written()
{
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    plan(&e, "");
    std::fs::remove_file(e.workspace.join(GAP_LOG)).unwrap();
    v2::start(&e, "claude", THREAD);
    // A chat miss writes gap logs, but a plan with no log and no rows is not
    // taken over.
    answer(
        &e,
        "miss",
        "b is 7",
        json!({"label":"sign of b","question":"b in x² − 7x?","first_try":"wrong","help":"none","cold":false}),
    );
    assert!(!e.workspace.join(GAP_LOG).exists());
    // A log written later, with a row, is brought in when a paper stage loads.
    std::fs::write(
        e.workspace.join(GAP_LOG),
        "## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Conf | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|---|\n| 1 | 2026-10-20 | Diagnostic | 3 | km | slope | K |  | taught | 2026-10-21 pass |  | 1 of 3 |\n",
    )
    .unwrap();
    let load = v2::tool(
        &e,
        "claude",
        THREAD,
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers_loop"}),
    );
    assert_eq!(load["gap_logs"]["imported"], json!([GAP_LOG]), "{load}");
    // Part-way through its redos with no date, it is due now, not never.
    assert_eq!(digest(&e)["plans"][0]["redos_due"], 1);
    // A file that is no longer text is left alone, whatever changes.
    let bytes = [b'#', b' ', 0xff, 0xfe, b'\n'];
    std::fs::write(e.workspace.join(GAP_LOG), bytes).unwrap();
    step(
        &e,
        json!({"schema":"learner-model/v2","action":"update","request_id":"repair",
            "practice":[{"item":"midterm-1#1","next":{"question":"Find Km.","answer_key":"2"}}]}),
    );
    assert_eq!(std::fs::read(e.workspace.join(GAP_LOG)).unwrap(), bytes);
}
