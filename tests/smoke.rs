use std::fs;

use support::TestEnvironment;

#[test]
fn native_binary_reports_version_without_touching_personal_state() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");

    let output = environment
        .command()
        .arg("--version")
        .output()
        .expect("run magnesium binary");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"magnesium 0.2.0\n");
    assert!(output.stderr.is_empty());
    assert!(environment.workspace.join("lesson.md").is_file());
    assert!(directory_is_empty(&environment.global_data));
    assert!(directory_is_empty(&environment.codex_home));
}

#[test]
fn help_lists_only_the_reserved_public_commands() {
    let environment = TestEnvironment::from_fixture("nested").expect("create test environment");
    let output = environment
        .command()
        .arg("--help")
        .output()
        .expect("run magnesium help");

    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).expect("help is UTF-8");
    for command in ["init", "memory", "update", "info", "digest", "export"] {
        assert!(help.contains(command), "help omitted {command}");
    }
    for forbidden in ["_internal", "chat", "trace", "unlink", "link"] {
        assert!(!help.contains(forbidden), "help exposed {forbidden}");
    }
    assert!(environment.workspace.join("child/topic.md").is_file());
    assert!(directory_is_empty(&environment.global_data));
    assert!(directory_is_empty(&environment.codex_home));
}

#[test]
fn update_prints_source_steps_and_writes_nothing_before_init() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let lesson_before = fs::read(environment.workspace.join("lesson.md")).expect("read fixture");
    let output = environment
        .command()
        .arg("update")
        .output()
        .expect("run update command");

    assert!(output.status.success(), "update failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    for step in [
        "git fetch --tags",
        "git checkout <newest-tag>",
        "git tag --sort=-v:refname | head -1",
        "cargo build --locked --release",
        "magnesium --version",
        "magnesium init --upgrade-instructions",
    ] {
        assert!(stdout.contains(step), "update omitted `{step}`: {stdout}");
    }
    let positions: Vec<usize> = [
        "git fetch --tags",
        "git tag --sort=-v:refname | head -1",
        "git checkout <newest-tag>",
        "cargo build --locked --release",
    ]
    .iter()
    .filter_map(|step| stdout.find(step))
    .collect();
    assert!(
        positions.is_sorted(),
        "update steps are out of run order: {stdout}"
    );
    assert!(
        !stdout.contains("git pull"),
        "update must check out a release tag, not pull main: {stdout}"
    );
    assert!(stdout.contains("No Magnesium plugins are installed yet"));
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read(environment.workspace.join("lesson.md")).expect("read unchanged fixture"),
        lesson_before
    );
    assert!(!environment.workspace.join(support::CONTROL_DIR).exists());
    assert!(!environment.data_dir().exists());
    assert!(directory_is_empty(&environment.global_data));
    assert!(directory_is_empty(&environment.codex_home));
}

fn directory_is_empty(path: &std::path::Path) -> bool {
    fs::read_dir(path)
        .expect("read isolated directory")
        .next()
        .is_none()
}
