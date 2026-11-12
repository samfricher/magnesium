//! The lesson's marks against a runtime from before the `attempts` command.
//! It is a test file of its own because it points the window at a stand-in
//! program through the environment, which no other test may share.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use mg_core::runtime;

fn stand_in(dir: &std::path::Path, script: &str) {
    let program = dir.join("magnesium");
    std::fs::write(&program, script).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var(runtime::PROGRAM_ENV, &program);
}

#[test]
fn a_runtime_without_attempts_has_no_marks_and_no_error_and_one_with_it_has_its_rows() {
    let dir = tempfile::tempdir().unwrap();

    // Before the command: a complaint on stderr and a failing exit.
    stand_in(
        dir.path(),
        "#!/bin/sh\necho \"error: unknown command: $1\" >&2\nexit 2\n",
    );
    assert_eq!(runtime::marks(dir.path()), None);

    // A program that prints something else on stdout and succeeds.
    stand_in(dir.path(), "#!/bin/sh\necho 'Usage: magnesium <command>'\n");
    assert_eq!(runtime::marks(dir.path()), None);

    // A runtime with the command and no answers yet.
    stand_in(dir.path(), "#!/bin/sh\necho '{\"attempts\": []}'\n");
    assert_eq!(runtime::marks(dir.path()), Some(Vec::new()));

    // One with a row.
    stand_in(
        dir.path(),
        "#!/bin/sh\necho '{\"attempts\": [{\"attempt_id\": \"a-1\", \"answered_at\": \"2026-11-12 09:41\", \"mark\": \"right\", \"help\": \"none\", \"cold\": true, \"label\": \"sign of b\"}]}'\n",
    );
    let marks = runtime::marks(dir.path()).unwrap();
    assert_eq!(marks.len(), 1);
    assert_eq!((marks[0].mark.as_str(), marks[0].cold), ("right", true));

    // No program at all.
    std::env::set_var(runtime::PROGRAM_ENV, dir.path().join("missing"));
    assert_eq!(runtime::marks(dir.path()), None);
}
