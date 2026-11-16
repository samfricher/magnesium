//! `magnesium calendar` prints the plans' planned sessions as an iCalendar
//! file, reports the lines it cannot read on standard error, and writes
//! nothing.
use crate::support::{TestEnvironment, v2};
use serde_json::Value;
use std::fs;
use std::path::Path;

fn write_plan(root: &Path, target: &str, sessions: &str) {
    let dir = root.join("Study").join(target);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plan.md"),
        format!(
            "# Midterm 1: study plan\n\n## 1. Facts\n\n| Item | Value |\n|---|---|\n| Target | Midterm 1 |\n| When | 2999-01-02 |\n\n## 14. Sessions\n\n{sessions}"
        ),
    )
    .unwrap();
}

fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.push((path.display().to_string(), fs::read(&path).unwrap()));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn calendar_prints_one_weekly_event_per_session_line_and_changes_nothing() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    write_plan(
        &e.workspace,
        "midterm-1",
        "- If it is Monday to Friday at 08:00, then I do the due cold redos at my desk.\n\
         - If it is Sunday at 7pm, then I read the gap log.\n\
         - If I miss a morning, then I do them after dinner.\n",
    );
    let before = tree(&e.workspace);

    let output = e.command().arg("calendar").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        tree(&e.workspace),
        before,
        "magnesium calendar changed the folder"
    );
    let file = String::from_utf8(output.stdout).unwrap();
    assert!(
        file.starts_with("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n"),
        "{file}"
    );
    assert!(file.ends_with("END:VCALENDAR\r\n"));
    assert_eq!(
        file.matches("BEGIN:VEVENT").count(),
        1,
        "the backup and the line it cannot read are no events"
    );
    assert!(
        file.contains("\r\nSUMMARY:Do the due cold redos at my desk\r\n"),
        "{file}"
    );
    assert!(
        file.contains(
            "\r\nRRULE:FREQ=WEEKLY;WKST=MO;BYDAY=MO,TU,WE,TH,FR;UNTIL=29990102T235959\r\n"
        ),
        "{file}"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("Study/midterm-1/plan.md, line ")
            && stderr.contains("not read, because the start time"),
        "{stderr}"
    );

    let again = e.command().arg("calendar").output().unwrap();
    let uid = |text: &str| {
        text.lines()
            .find(|l| l.starts_with("UID:"))
            .map(str::to_owned)
    };
    assert_eq!(
        uid(&file),
        uid(&String::from_utf8(again.stdout).unwrap()),
        "the same session keeps its UID"
    );

    let listed = e.command().args(["calendar", "--json"]).output().unwrap();
    assert!(listed.status.success(), "{listed:?}");
    let value: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(
        value["sessions"][0]["action"],
        "Do the due cold redos at my desk"
    );
    assert_eq!(value["sessions"][0]["start"], "08:00");
    assert_eq!(value["unreadable"].as_array().unwrap().len(), 1);
}

#[test]
fn calendar_with_no_planned_session_prints_nothing_and_says_why() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    write_plan(&e.workspace, "midterm-1", "- none planned\n");
    let output = e.command().arg("calendar").output().unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("no planned sessions")
    );
    let listed = e.command().args(["calendar", "--json"]).output().unwrap();
    let value: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(value["sessions"], serde_json::json!([]));
}
