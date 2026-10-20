use std::fs;
use std::path::Path;
use std::process::Command;

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
        "git tag --sort=-v:refname",
        "git checkout <newest-tag>",
        "detached HEAD is expected",
        "git describe --tags --exact-match",
        "cargo build --locked --release",
        "magnesium --version",
        "magnesium init --upgrade-instructions",
    ] {
        assert!(stdout.contains(step), "update omitted `{step}`: {stdout}");
    }
    let positions: Vec<usize> = [
        "git fetch --tags",
        "git tag --sort=-v:refname",
        "git checkout <newest-tag>",
        "git describe --tags --exact-match",
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

#[test]
fn update_tag_lookup_picks_the_newest_release_and_skips_suffixed_tags() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let output = environment
        .command()
        .arg("update")
        .output()
        .expect("run update command");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let lookup = stdout
        .split('`')
        .find(|span| span.starts_with("git tag "))
        .unwrap_or_else(|| panic!("update printed no tag lookup: {stdout}"))
        .to_owned();
    let guide =
        fs::read_to_string(support::repo_root().join("docs/install.md")).expect("read guide");
    assert!(guide.contains(&lookup), "docs/install.md lacks `{lookup}`");

    let clone = environment.root.join("release-clone");
    fs::create_dir(&clone).expect("create clone folder");
    let git = |args: &[&str]| {
        let status = isolated(&clone, "git")
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&[
        "-c",
        "user.name=test",
        "-c",
        "user.email=test@example.invalid",
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "release",
    ]);
    for tag in [
        "v0.9.0",
        "v0.10.0",
        "v0.11.0-rc1",
        "v0.11.0-rc.1",
        "v0.11.0",
        "v0.12.0-rc.1",
    ] {
        git(&["tag", tag]);
    }

    let newest = isolated(&clone, "sh")
        .args(["-c", &lookup])
        .output()
        .expect("run the printed tag lookup");
    assert!(newest.status.success(), "{lookup} failed: {newest:?}");
    assert_eq!(
        String::from_utf8_lossy(&newest.stdout),
        "v0.11.0\n",
        "{lookup}"
    );
}

fn isolated(directory: &Path, program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", directory.join("absent-global-config"));
    command
}

fn directory_is_empty(path: &std::path::Path) -> bool {
    fs::read_dir(path)
        .expect("read isolated directory")
        .next()
        .is_none()
}
