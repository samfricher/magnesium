//! While the study window's daily notification is on, the window names its
//! program, and each change the runtime commits to the record starts it so it
//! can schedule the notification again.
use crate::support::{TestEnvironment, v2};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const THREAD: &str = "notifier";

/// A stand-in for the window's program, inside an app bundle, that notes each start.
fn stand_in(e: &TestEnvironment) -> (PathBuf, PathBuf) {
    let contents = e.root.join("Magnesium.app/Contents");
    fs::create_dir_all(contents.join("MacOS")).unwrap();
    fs::write(contents.join("Info.plist"), "<plist/>").unwrap();
    let marker = e.root.join("started.txt");
    let program = contents.join("MacOS/magnesium-window");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\necho \"$* runtime=$MAGNESIUM_APP_RUNTIME\" >> '{}'\n",
            marker.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    }
    (program, marker)
}

fn starts(marker: &Path, at_least: usize) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let lines = fs::read_to_string(marker)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if lines.len() >= at_least || Instant::now() > deadline {
            return lines;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn capture(e: &TestEnvironment, request_id: &str, revision: i64, session: &str) {
    let receipt = v2::tool(
        e,
        "claude",
        THREAD,
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":request_id,
            "session":{"operation":"use","session_id":session,"expected_revision":revision},
            "reported_events":[{"external_event_key":request_id,"speaker":"learner","kind":"message",
                "text":"In 2x² + 7x + 3, b = 7.","occurred_at_ms":1}]}),
    );
    assert_eq!(receipt["update_status"], "committed", "{receipt}");
}

#[test]
fn a_committed_change_starts_the_named_window_program_and_off_stops_it() {
    let e = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&e);
    let (program, marker) = stand_in(&e);
    let started = v2::start(&e, "claude", THREAD);
    let session = started["references"]["session"]
        .as_str()
        .unwrap()
        .to_owned();

    // Off: nothing is started.
    capture(&e, "before", 1, &session);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!marker.exists(), "nothing was named, so nothing started");

    let home = e.root.join("window");
    fs::create_dir_all(&home).unwrap();
    let named = e
        .command()
        .args(["_internal", "window-notifier"])
        .arg(&program)
        .arg("--home")
        .arg(&home)
        .output()
        .unwrap();
    assert!(named.status.success(), "{named:?}");
    capture(&e, "named", 2, &session);
    let started = starts(&marker, 1);
    assert_eq!(started.len(), 1, "{started:?}");
    assert!(
        started[0].starts_with(&format!(
            "--refresh-due-notifications {} runtime=/",
            home.display()
        )),
        "it is started with the window's folder and this runtime: {started:?}"
    );
    assert!(started[0].ends_with("magnesium"), "{started:?}");

    let off = e
        .command()
        .args(["_internal", "window-notifier", "--off"])
        .output()
        .unwrap();
    assert!(off.status.success(), "{off:?}");
    capture(&e, "after", 3, &session);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(starts(&marker, 1).len(), 1, "off: no further start");

    // A program outside an app bundle is refused.
    let loose = e.root.join("loose.sh");
    fs::write(&loose, "#!/bin/sh\n").unwrap();
    let refused = e
        .command()
        .args(["_internal", "window-notifier"])
        .arg(&loose)
        .output()
        .unwrap();
    assert!(!refused.status.success());
}
