use super::*;

const PLAN: &str = "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | 2026-10-23 18:00 |\n| Length | <minutes> |\n\n## 6. Blocks\n\n| # | Block | Purpose |\n|---|---|---|\n| 1 | Diagnostic | find gaps |\n| 2 | Cold redo | close gaps |\n\n## 7. Must know cold\n\n- Km\n";
const GAPS: &str = "# Gap log\n\n## Gaps\n\n| # | Date | Paper | Status |\n|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic | open |\n| 2 | 2026-10-14 | Diagnostic | closed |\n| 3 | 2026-10-14 | Diagnostic | Open |\n\n## Scores\n\n| Paper | Date | Score | Time used |\n|---|---|---|---|\n| Diagnostic | 2026-10-14 | 14/30 | 40/45 min |\n| <PAPER> | <YYYY-MM-DD> | <score> | |\n";

#[test]
fn plan_digest_reads_facts_blocks_gaps_and_scores() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
    let value = plan_digest("midterm-1", PLAN, GAPS, today, &[]);
    assert_eq!(value["title"], "Midterm 1");
    assert_eq!(value["date"], "2026-10-23");
    assert_eq!(value["days_left"], 7);
    assert_eq!(value["blocks"], json!(["Diagnostic", "Cold redo"]));
    assert_eq!(value["unfixed_gaps"], 2);
    assert_eq!(
        value["scores"],
        json!([{"paper":"Diagnostic","date":"2026-10-14","score":"14/30"}])
    );
    assert_eq!(value["session"], Value::Null);
}

#[test]
fn reshaped_tables_are_read_by_header_name() {
    let plan = "## 1. Facts\n\n| Item | Value |\n|---|---|\n| When | Wed Oct 21, 12:30 |\n\n## 6. Blocks\n\n| # | Day | Block | Status |\n|---|---|---|---|\n| 1 | Fri 16 | Diagnostic | done |\n| 2 | Sat 17 | Cold redo | |\n";
    let gaps = "## Gaps\n\n| # | Date | Paper | Status | Notes |\n|---|---|---|---|---|\n| 1 | 2026-10-15 | Diagnostic | open | teach first |\n| 2 | 2026-10-15 | Diagnostic | closed | |\n\n## Scores\n\n| Date | Paper | Time used | Score |\n|---|---|---|---|\n| 2026-10-15 | Diagnostic | 44/45 min | 18/30 |\n";
    let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
    let value = plan_digest("midterm-1", plan, gaps, today, &[]);
    assert_eq!(value["blocks"], json!(["Diagnostic", "Cold redo"]));
    assert_eq!(value["unfixed_gaps"], 1);
    assert_eq!(
        value["scores"],
        json!([{"paper":"Diagnostic","date":"2026-10-15","score":"18/30"}])
    );
    // The papers skill writes the When cell with its ISO date first; other forms give no date.
    assert_eq!(value["when"], "Wed Oct 21, 12:30");
    assert_eq!(value["date"], Value::Null);
    assert_eq!(value["days_left"], Value::Null);
}

#[test]
fn template_placeholders_are_not_facts() {
    let template = include_str!("../../claude-plugin/skills/papers/templates/plan.md");
    let gaps = include_str!("../../claude-plugin/skills/papers/templates/gap-log.md");
    let today = NaiveDate::from_ymd_opt(2026, 10, 16).unwrap();
    let value = plan_digest("final", template, gaps, today, &[]);
    assert_eq!(value["title"], Value::Null);
    assert_eq!(value["date"], Value::Null);
    assert_eq!(value["blocks"], json!([]));
    assert_eq!(value["unfixed_gaps"], 0);
    assert_eq!(value["redos_due"], 0);
    assert_eq!(value["first_due"], Value::Null);
    assert_eq!(value["scores"], json!([]));
    assert_eq!(value["planned_sessions_today"], json!([]));
    assert_eq!(value["unreadable"], json!([]));
}

#[test]
fn a_missing_sessions_section_leaves_the_rest_of_the_digest() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let without = plan_digest("midterm-1", PLAN, GAPS, today, &[]);
    assert_eq!(without["planned_sessions_today"], json!([]));
    assert_eq!(without["unfixed_gaps"], 2);
    let plan_items = without["unreadable"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["file"] == "Study/midterm-1/plan.md")
        .count();
    assert_eq!(plan_items, 0);
}

#[test]
fn sessions_lines_it_cannot_read_are_reported_not_treated_as_no_plan() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let mut without = plan_digest("midterm-1", PLAN, GAPS, today, &[]);
    without["unreadable"] = json!([]);
    let plan = format!(
        "{PLAN}\n## 14. Sessions\n\n- If it is Tuesday at 8am, then I do the due cold redos.\n"
    );
    let value = plan_digest("midterm-1", &plan, GAPS, today, &[]);
    let items = value["unreadable"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["file"] == "Study/midterm-1/plan.md")
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 1, "{value}");
    assert_eq!(items[0]["section"], "Sessions");
    let line = plan.lines().position(|line| line.contains("8am")).unwrap() + 1;
    assert_eq!(items[0]["line"], line);
    let mut rest = value.clone();
    rest["unreadable"] = json!([]);
    assert_eq!(rest, without);
}

#[test]
fn unreadable_gap_rows_and_sessions_lines_are_listed_together() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let gaps = "## Gaps\n\n| # | Date | Paper | Next redo | Status |\n|---|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic |\n";
    let plan = format!(
        "{PLAN}\n## 14. Sessions\n\n- If it is Tuesday at 8am, then I do the due cold redos.\n"
    );
    let value = plan_digest("midterm-1", &plan, gaps, today, &[]);
    let files = value["unreadable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["file"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        files,
        ["Study/midterm-1/gap-log.md", "Study/midterm-1/plan.md"]
    );
}

#[test]
fn a_gap_is_fixed_only_when_its_status_says_closed() {
    let gaps = "## Gaps\n\n| # | Date | Paper | Q | Next redo | Status |\n|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-21 | open |\n\
            | 2 | 2026-10-14 | Diagnostic | Q2 | 2026-10-20 | Open |\n\
            | 3 | 2026-10-14 | Diagnostic | Q3 | 2026-10-19 | 1 of 3 |\n\
            | 4 | 2026-10-14 | Diagnostic | Q6 | | open (failed) |\n\
            | 5 | 2026-10-12 | Paper1-Easy | Q4 | 2026-10-16 | 1 of 3 passes |\n\
            | 6 | 2026-10-12 | Paper1-Easy | Q6 | 2026-10-18 | reopened |\n\
            | 7 | 2026-10-12 | Paper1-Easy | Q7 | 2026-10-25 | 0 of 3 |\n\
            | 8 | 2026-10-12 | Paper1-Easy | Q8 | 2026-10-17 | |\n\
            | 9 | 2026-10-08 | Diagnostic | Q5 | 2026-10-01 | closed |\n\
            | 10 | 2026-10-08 | Diagnostic | Q7 | 2026-10-01 | **Closed** |\n\
            | 11 | 2026-10-08 | Diagnostic | Q8 | 2026-10-01 | CLOSED |\n\
            | 12 | 2026-10-08 | Diagnostic | Q9 | 2026-10-01 | 3 of 3 |\n";
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
    assert_eq!(value["unfixed_gaps"], 8);
    assert_eq!(value["redos_due"], 5);
    assert_eq!(
        value["first_due"],
        json!({"paper": "Paper1-Easy", "question": "Q4", "more_due": 4})
    );
}

fn unfixed_for_status(status: &str) -> Value {
    let gaps = format!(
        "## Gaps\n\n| # | Date | Paper | Q | Next redo | Status |\n|---|---|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-19 | {status} |\n"
    );
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    plan_digest("midterm-1", PLAN, &gaps, today, &[])["unfixed_gaps"].clone()
}

#[test]
fn only_an_exact_closed_or_3_of_3_status_closes_a_gap() {
    for status in [
        "closed",
        "Closed",
        "CLOSED",
        "**Closed**",
        "_closed_",
        "3 of 3",
        "**3 of 3**",
    ] {
        assert_eq!(
            unfixed_for_status(status),
            0,
            "{status:?} should close the gap"
        );
    }
}

#[test]
fn every_other_status_keeps_a_gap_unfixed() {
    for status in [
        "",
        "open",
        "1 of 3",
        "2 of 3",
        "2/3",
        "1 of 3 passes",
        "0 of 3",
        "open (failed)",
        "open (repair pending)",
        "reopened",
        "not closed",
        "isn't closed",
        "~~closed~~",
        "~~closed~~ open",
        "~closed~",
        "<del>closed</del>",
        "closed, then failed 2026-10-24",
        "1 of 3 done",
        "2/3 done",
        "1 of 3 (repair done)",
        "2 of 3 (3 of 3 due 2026-10-25)",
        "Closed ✓",
        "closed by MockA Q2",
        "done",
        "3/3",
        "3 of 3.",
        "3 of 3 passes",
        "3 of 3 (closed)",
        "3 out of 3",
        "~~2 of 3~~ closed",
    ] {
        assert_eq!(
            unfixed_for_status(status),
            1,
            "{status:?} should leave the gap unfixed"
        );
    }
}

#[test]
fn an_upgraded_old_log_keeps_its_overdue_row_due() {
    let gaps = "## Gaps\n\n| # | Date | Paper | Q | Tag | What went wrong | Type | Repair done | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|---|---|---|---|\n\
            | 1 | 2026-10-01 | Diagnostic | Q1 | km | sign error | A | tighten | | 2026-10-03 | OPEN |\n\
            | 2 | 2026-10-01 | Diagnostic | Q2 | vmax | no units | N | tighten | | | closed |\n\
            | 3 | 2026-10-08 | MockA | Q4 | inhibition | wrong type | K | repair | | 2026-10-25 | open |\n";
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let value = plan_digest("final", PLAN, gaps, today, &[]);
    assert_eq!(value["unfixed_gaps"], 2);
    assert_eq!(value["redos_due"], 1);
    assert_eq!(
        value["first_due"],
        json!({"paper": "Diagnostic", "question": "Q1", "more_due": 0})
    );
}

#[test]
fn redos_due_counts_unfixed_rows_whose_next_redo_has_come() {
    let gaps = "## Gaps\n\n| Status | # | Q | Paper | NEXT REDO | Date |\n|---|---|---|---|---|---|\n\
            | open | 1 | Q2 | Diagnostic | 2026-10-21 | 2026-10-14 |\n\
            | 1 of 3 | 2 | Q4 | Diagnostic | 2026-10-18 | 2026-10-14 |\n\
            | 2 of 3 | 3 | Q1 | Paper1-Easy | 2026-10-20 (Tue) | 2026-10-16 |\n\
            | closed | 4 | Q5 | Diagnostic | 2026-10-01 | 2026-10-14 |\n\
            | open | 5 | Q6 | Diagnostic | | 2026-10-14 |\n\
            | open | 6 | Q2 | MockA | 2026-10-18 | 2026-10-17 |\n";
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
    assert_eq!(value["unfixed_gaps"], 5);
    assert_eq!(value["redos_due"], 3);
    assert_eq!(
        value["first_due"],
        json!({"paper": "Diagnostic", "question": "Q4", "more_due": 2})
    );
}

fn four_gap_rows_in_columns(columns: &[&str]) -> String {
    let names = ["Q", "Type", "Conf", "Passes", "Next redo", "Status"];
    let rows = [
        ["Q1", "A", "S", "", "2026-10-16", "open"],
        ["Q2", "N", "", "2026-10-16 pass", "2026-10-19", "1 of 3"],
        [
            "Q4",
            "K",
            "G",
            "2026-10-15 pass, 2026-10-18 pass, 2026-10-25 pass",
            "",
            "closed",
        ],
        ["Q5", "K", "U", "2026-10-17 fail", "", "open"],
    ];
    let line = |cells: Vec<&str>| format!("| {} |\n", cells.join(" | "));
    let mut text = String::from("## Gaps\n\n");
    text += &line(columns.to_vec());
    text += &line(columns.iter().map(|_| "---").collect());
    for row in rows {
        text += &line(
            columns
                .iter()
                .map(|column| match *column {
                    "Date" => "2026-10-14",
                    "Paper" => "Diagnostic",
                    name => row[names.iter().position(|n| *n == name).unwrap()],
                })
                .collect(),
        );
    }
    text
}

#[test]
fn a_conf_column_in_any_position_leaves_the_digest_unchanged() {
    const SCORES: &str = "\n## Scores\n\n| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n\
            | Diagnostic | 2026-10-14 | 18/30 | 44/45 min | Q1 2/4 |\n";
    const SCORES_WITH_CONF_COUNTS: &str = "\n## Scores\n\n| Paper | Date | Score | Time used | Notes |\n|---|---|---|---|---|\n\
            | Diagnostic | 2026-10-14 | 18/30 | 44/45 min | Q1 2/4; Conf right: S 9/12, U 4/6, G 1/2 |\n";
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let without = [
        "Date",
        "Paper",
        "Q",
        "Type",
        "Passes",
        "Next redo",
        "Status",
    ];
    let expected = plan_digest(
        "midterm-1",
        PLAN,
        &(four_gap_rows_in_columns(&without) + SCORES),
        today,
        &[],
    );
    assert_eq!(expected["unfixed_gaps"], 3);
    assert_eq!(expected["redos_due"], 2);
    assert_eq!(
        expected["first_due"],
        json!({"paper": "Diagnostic", "question": "Q1", "more_due": 1})
    );
    assert_eq!(
        expected["scores"],
        json!([{"paper":"Diagnostic","date":"2026-10-14","score":"18/30"}])
    );
    for at in 0..=without.len() {
        let mut columns = without.to_vec();
        columns.insert(at, "Conf");
        assert_eq!(
            plan_digest(
                "midterm-1",
                PLAN,
                &(four_gap_rows_in_columns(&columns) + SCORES_WITH_CONF_COUNTS),
                today,
                &[]
            ),
            expected,
            "Conf at column {at}"
        );
    }
}

const CONF_COLUMNS: [&str; 8] = [
    "Date",
    "Paper",
    "Q",
    "Type",
    "Conf",
    "Passes",
    "Next redo",
    "Status",
];

fn unreadable_rows(value: &Value) -> Vec<(u64, &str)> {
    value["unreadable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert_eq!(item["file"], "Study/midterm-1/gap-log.md");
            assert_eq!(item["section"], "Gaps");
            (
                item["line"].as_u64().unwrap(),
                item["reason"].as_str().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_closed_row_missing_its_conf_cell_is_reported_and_does_not_become_unfixed() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let full = four_gap_rows_in_columns(&CONF_COLUMNS);
    let short = full.replace("| K | G | ", "| K | ");
    assert_ne!(short, full);
    let value = plan_digest("midterm-1", PLAN, &full, today, &[]);
    assert_eq!(value["unfixed_gaps"], 3);
    assert_eq!(value["unreadable"], json!([]));
    let value = plan_digest("midterm-1", PLAN, &short, today, &[]);
    assert_eq!(value["unfixed_gaps"], 3);
    assert_eq!(value["redos_due"], 2);
    let rows = unreadable_rows(&value);
    assert_eq!(rows.len(), 1, "{value}");
    assert_eq!(rows[0].0, 7);
    assert!(
        rows[0].1.contains('7') && rows[0].1.contains('8'),
        "{}",
        rows[0].1
    );
}

#[test]
fn an_open_due_row_missing_its_conf_cell_is_reported_not_silently_dropped() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let full = four_gap_rows_in_columns(&CONF_COLUMNS);
    let short = full.replace("| A | S | ", "| A | ");
    assert_ne!(short, full);
    let value = plan_digest("midterm-1", PLAN, &short, today, &[]);
    let rows = unreadable_rows(&value);
    assert_eq!(rows.len(), 1, "{value}");
    assert_eq!(rows[0].0, 5);
    assert_eq!(value["unfixed_gaps"], 2);
    assert_eq!(value["redos_due"], 1);
    assert_eq!(
        value["first_due"],
        json!({"paper": "Diagnostic", "question": "Q2", "more_due": 0})
    );
}

#[test]
fn rows_with_conf_cells_under_a_header_without_conf_are_all_reported_and_none_counted() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let full = four_gap_rows_in_columns(&CONF_COLUMNS);
    let headless = full.replace("| Type | Conf |", "| Type |");
    assert_ne!(headless, full);
    let value = plan_digest("midterm-1", PLAN, &headless, today, &[]);
    let lines = unreadable_rows(&value)
        .into_iter()
        .map(|(line, _)| line)
        .collect::<Vec<_>>();
    assert_eq!(lines, [5, 6, 7, 8]);
    assert_eq!(value["unfixed_gaps"], 0);
    assert_eq!(value["redos_due"], 0);
    assert_eq!(value["first_due"], Value::Null);
}

const GAP_HEADER: &str = "# Gap log\n\n## Gaps\n\n| # | Date | Paper | Q | Passes | Next redo | Status |\n|---|---|---|---|---|---|---|\n";

fn digest_of_rows(rows: &[&str]) -> Value {
    let gaps = format!("{GAP_HEADER}{}\n", rows.join("\n"));
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    plan_digest("midterm-1", PLAN, &gaps, today, &[])
}

fn unreadable_lines(value: &Value) -> Vec<(&str, u64)> {
    value["unreadable"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            assert_eq!(item["file"], "Study/midterm-1/gap-log.md");
            (
                item["section"].as_str().unwrap(),
                item["line"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_readable_gap_log_reports_nothing_unreadable() {
    let value = digest_of_rows(&[
        "| 1 | 2026-10-14 | Diagnostic | Q1 | | 2026-10-19 | open |",
        "| 2 | 2026-10-14 | Diagnostic | Q2 | | | open |",
    ]);
    assert_eq!(value["unreadable"], json!([]));
    assert_eq!(value["unfixed_gaps"], 2);
    assert_eq!(value["redos_due"], 1);
}

#[test]
fn a_next_redo_that_is_not_a_date_is_reported_and_left_out_of_the_counts() {
    let value = digest_of_rows(&[
        "| 1 | 2026-10-14 | Diagnostic | Q1 | | soon | open |",
        "| 2 | 2026-10-14 | Diagnostic | Q2 | | 2026-10-19 | open |",
        "| 3 | 2026-10-14 | Diagnostic | Q3 | | 2026-10-4 | 1 of 3 |",
    ]);
    assert_eq!(unreadable_lines(&value), [("Gaps", 7), ("Gaps", 9)]);
    let reason = value["unreadable"][0]["reason"].as_str().unwrap();
    assert!(
        reason.contains("Next redo") && reason.contains("soon"),
        "{reason}"
    );
    assert_eq!(value["unfixed_gaps"], 1);
    assert_eq!(value["redos_due"], 1);
    assert_eq!(
        value["first_due"],
        json!({"paper": "Diagnostic", "question": "Q2", "more_due": 0})
    );
}

#[test]
fn a_closed_row_is_not_read_for_its_next_redo() {
    let value = digest_of_rows(&["| 1 | 2026-10-14 | Diagnostic | Q1 | | n/a | closed |"]);
    assert_eq!(value["unreadable"], json!([]));
    assert_eq!(value["unfixed_gaps"], 0);
}

#[test]
fn a_row_with_too_few_or_too_many_cells_is_reported_and_left_out_of_the_counts() {
    let value = digest_of_rows(&[
        "| 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-19 | open |",
        "| 2 | 2026-10-14 | Diagnostic | Q2 | | 2026-10-19 | open |",
        "| 3 | 2026-10-14 | Diagnostic | Q3 | | 2026-10-19 | open | extra |",
        "| 4 | 2026-10-14 | Diagnostic | Q4 | | 2026-10-19 | closed |",
    ]);
    assert_eq!(unreadable_lines(&value), [("Gaps", 7), ("Gaps", 9)]);
    let reason = value["unreadable"][0]["reason"].as_str().unwrap();
    assert!(reason.contains('6') && reason.contains('7'), "{reason}");
    assert_eq!(value["unfixed_gaps"], 1);
    assert_eq!(value["redos_due"], 1);
    assert_eq!(
        value["first_due"],
        json!({"paper": "Diagnostic", "question": "Q2", "more_due": 0})
    );
}

#[test]
fn escaped_pipes_and_an_empty_last_cell_do_not_misalign_a_row() {
    let gaps = "## Gaps\n\n| # | Date | Q | Next redo | Status | Notes |\n|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Q1 | 2026-10-19 | open | a \\| b |\n\
            | 2 | 2026-10-14 | Q2 | 2026-10-19 | open | |\n\
            | 3 | 2026-10-14 | Q3 | 2026-10-19 | open |  |\n\
            | 4 | 2026-10-14 | Q4 | 2026-10-19 | open\n";
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
    assert_eq!(unreadable_lines(&value), [("Gaps", 8)]);
    assert_eq!(value["unfixed_gaps"], 3);
    assert_eq!(value["redos_due"], 3);
}

#[test]
fn rows_after_a_break_in_the_gaps_table_are_reported_and_left_out_of_the_counts() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let gaps = "## Gaps\n\n| # | Date | Paper | Q | Next redo | Status |\n|---|---|---|---|---|---|\n\
            | 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-19 | open |\n\
            \n\
            | 2 | 2026-10-14 | Diagnostic | Q2 | 2026-10-18 | open |\n\
            ### Paper1-Easy\n\
            | # | Date | Paper | Q | Next redo | Status |\n|---|---|---|---|---|---|\n\
            | 3 | 2026-10-15 | Paper1-Easy | Q4 | 2026-10-17 | 1 of 3 |\n\
            \n\
            ## Scores\n\n| Paper | Date | Score |\n|---|---|---|\n";
    let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
    assert_eq!(
        unreadable_lines(&value),
        [("Gaps", 7), ("Gaps", 9), ("Gaps", 11)]
    );
    let reason = value["unreadable"][0]["reason"].as_str().unwrap();
    assert!(reason.contains("break"), "{reason}");
    assert_eq!(value["unfixed_gaps"], 1);
    assert_eq!(value["redos_due"], 1);
    assert_eq!(
        value["first_due"],
        json!({"paper": "Diagnostic", "question": "Q1", "more_due": 0})
    );

    let only_after_a_break = "## Gaps\n\n| # | Date | Next redo | Status |\n|---|---|---|---|\n\n| 1 | 2026-10-14 | 2026-10-19 | open |\n";
    let value = plan_digest("midterm-1", PLAN, only_after_a_break, today, &[]);
    assert_eq!(unreadable_lines(&value), [("Gaps", 6)]);
    assert_eq!(value["unfixed_gaps"], 0);
}

#[test]
fn a_row_without_a_date_is_reported_unless_it_is_the_template_row() {
    let value = digest_of_rows(&[
        "| 1 | <YYYY-MM-DD> | <PAPER> | <Q> | | <empty until the repair> | open |",
        "| 2 | | Diagnostic | Q2 | | 2026-10-19 | open |",
    ]);
    assert_eq!(unreadable_lines(&value), [("Gaps", 8)]);
    assert_eq!(value["unfixed_gaps"], 0);
    assert_eq!(value["redos_due"], 0);
}

#[test]
fn a_gaps_table_missing_a_column_the_digest_needs_is_reported_once() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let no_status = "# Gap log\n\n## Gaps\n\n| # | Date | Q | Next redo |\n|---|---|---|---|\n| 1 | 2026-10-14 | Q1 | 2026-10-19 |\n| 2 | 2026-10-14 | Q2 | 2026-10-19 |\n";
    let value = plan_digest("midterm-1", PLAN, no_status, today, &[]);
    assert_eq!(unreadable_lines(&value), [("Gaps", 5)]);
    assert!(
        value["unreadable"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("Status")
    );
    assert_eq!(value["unfixed_gaps"], 0);
    assert_eq!(value["redos_due"], 0);

    let no_date = "## Gaps\n\n| # | Q | Next redo | Status |\n|---|---|---|---|\n| 1 | Q1 | 2026-10-19 | open |\n";
    let value = plan_digest("midterm-1", PLAN, no_date, today, &[]);
    assert!(
        value["unreadable"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("Date")
    );
    assert_eq!(value["unfixed_gaps"], 0);
}

#[test]
fn an_old_log_without_a_next_redo_column_is_reported_with_its_gaps_still_counted() {
    let gaps = "## Gaps\n\n| # | Date | Paper | Q | Cold-redo date | Status |\n|---|---|---|---|---|---|\n| 1 | 2026-10-14 | Diagnostic | Q1 | 2026-10-15 | open |\n";
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
    assert_eq!(unreadable_lines(&value), [("Gaps", 3)]);
    assert!(
        value["unreadable"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("Next redo")
    );
    assert_eq!(value["unfixed_gaps"], 1);
    assert_eq!(value["redos_due"], 0);
}

#[test]
fn a_gaps_table_with_no_rows_reports_nothing() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
    for gaps in [
        "",
        "## Gaps\n\nNo gaps yet.\n",
        "## Gaps\n\n| # | Date |\n|---|---|\n",
    ] {
        let value = plan_digest("midterm-1", PLAN, gaps, today, &[]);
        assert_eq!(value["unreadable"], json!([]), "{gaps:?}");
    }
}

#[test]
fn the_gap_log_template_leaves_the_gaps_table_to_the_runtime() {
    let template = include_str!("../../claude-plugin/skills/papers/templates/gap-log.md");
    let gaps = section(template, "Gaps").text;
    let table = Table::parse(section(template, "Gaps"));
    assert!(table.rows.is_empty(), "no row for the tutor to fill in");
    for name in ["Passes", "Next redo", "Status", "Conf"] {
        assert!(table.column(name).is_some(), "{name}");
    }
    assert!(gaps.contains("Magnesium writes this table"));
}

#[test]
fn goals_match_a_target_by_slug_or_title() {
    assert!(names_target("Midterm 1 practice papers", "midterm-1", None));
    assert!(names_target("Study for ch6-7", "ch6-7", None));
    assert!(names_target(
        "BIO final review",
        "final",
        Some("Final exam")
    ));
    assert!(!names_target(
        "Week 4: enzyme kinetics",
        "midterm-1",
        Some("Midterm 1")
    ));
}

#[test]
fn long_text_is_clipped_on_a_character_boundary() {
    let long = "é".repeat(MAX_TEXT_CHARS + 5);
    let clipped = clip(&long);
    assert!(clipped.ends_with("..."));
    assert_eq!(clipped.chars().count(), MAX_TEXT_CHARS + 3);
}
