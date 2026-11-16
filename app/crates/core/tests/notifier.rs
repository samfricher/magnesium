//! The window tells the runtime which program to start after each recorded
//! answer, and finds out when the runtime cannot be told, so the notification
//! is never left on with nothing to keep it true.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use mg_core::{reminder, runtime};

fn script(path: &std::path::Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn naming_the_program_fails_with_a_runtime_that_lacks_the_command_and_succeeds_with_one_that_has_it(
) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("window");
    fs::create_dir_all(&home).unwrap();
    let program = dir
        .path()
        .join("Magnesium.app/Contents/MacOS/magnesium-window");

    // A runtime from before the command: clap refuses it, exit 2.
    let old = dir.path().join("old-magnesium");
    script(
        &old,
        "echo \"error: unrecognized subcommand 'window-notifier'\" >&2\nexit 2",
    );
    std::env::set_var(runtime::PROGRAM_ENV, &old);
    let refused = reminder::name_program(Some(&program), &home);
    assert!(refused.is_err(), "{refused:?}");
    assert!(
        reminder::name_program(None, &home).is_err(),
        "--off fails the same way"
    );

    // A runtime with it: the window's program, its folder, and --off.
    let seen = dir.path().join("args.txt");
    let new = dir.path().join("new-magnesium");
    script(&new, &format!("echo \"$*\" >> '{}'", seen.display()));
    std::env::set_var(runtime::PROGRAM_ENV, &new);
    reminder::name_program(Some(&program), &home).unwrap();
    reminder::name_program(None, &home).unwrap();
    let args = fs::read_to_string(&seen).unwrap();
    let lines: Vec<&str> = args.lines().collect();
    assert_eq!(
        lines,
        [
            format!(
                "_internal window-notifier {} --home {}",
                program.display(),
                home.display()
            )
            .as_str(),
            "_internal window-notifier --off",
        ]
    );

    // No runtime at all.
    std::env::set_var(runtime::PROGRAM_ENV, dir.path().join("missing"));
    assert!(reminder::name_program(Some(&program), &home).is_err());
}
