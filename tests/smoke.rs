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
    assert_eq!(
        output.stdout,
        format!("magnesium {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
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
        "git tag --points-at HEAD",
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
        "git tag --points-at HEAD",
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
    let printed = |prefix: &str| {
        stdout
            .split('`')
            .find(|span| span.starts_with(prefix))
            .unwrap_or_else(|| panic!("update printed no `{prefix}` command: {stdout}"))
            .to_owned()
    };
    let lookup = printed("git tag --sort");
    let check = printed("git tag --points-at");
    let guide =
        fs::read_to_string(support::repo_root().join("docs/install.md")).expect("read guide");
    for command in [&lookup, &check] {
        assert!(
            guide.contains(command.as_str()),
            "docs/install.md lacks `{command}`"
        );
    }

    // Built at run time, so the release step's search for the old version never rewrites them.
    let tag = |minor: u32, suffix: &str| format!("v0.{minor}.0{suffix}");
    let clone = environment.root.join("release-clone");
    fs::create_dir(&clone).expect("create clone folder");
    let git = |args: &[&str]| {
        let status = isolated(&clone, "git")
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    };
    let commit = |message: &str| git(&["commit", "--quiet", "--allow-empty", "-m", message]);
    git(&["init", "--quiet"]);
    commit("first release");
    git(&["tag", &tag(9, "")]);
    commit("second release");
    git(&["tag", &tag(10, "")]);
    commit("candidate promoted to a release");
    git(&["tag", "-a", "-m", "candidate", &tag(11, "-rc1")]);
    git(&["tag", "-a", "-m", "candidate", &tag(11, "-rc.1")]);
    git(&["tag", "-a", "-m", "build", &tag(11, "+build.1")]);
    git(&["tag", "-a", "-m", "channel", "stable"]);
    git(&["tag", &tag(11, "")]);
    commit("next candidate");
    git(&["tag", "-a", "-m", "candidate", &tag(12, "-rc.1")]);

    let run = |command: &str| {
        let output = isolated(&clone, "sh")
            .args(["-c", command])
            .output()
            .expect("run a printed command");
        assert!(output.status.success(), "{command} failed: {output:?}");
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_owned()
    };
    let newest = run(&lookup);
    assert_eq!(newest, tag(11, ""), "{lookup}");
    git(&["checkout", "--quiet", &newest]);
    assert_eq!(run(&check), newest, "{check}");
}

#[test]
fn tag_lookup_test_leaves_an_inherited_git_dir_untouched() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let repository = environment.root.join("developer-clone");
    fs::create_dir(&repository).expect("create repository folder");
    let refs = || {
        let output = isolated(&repository, "git")
            .args(["for-each-ref", "--format=%(refname) %(objectname)"])
            .output()
            .expect("list refs");
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    for args in [
        &["init", "--quiet"][..],
        &[
            "-c",
            "user.name=developer",
            "-c",
            "user.email=developer@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "developer work",
        ],
    ] {
        let status = isolated(&repository, "git")
            .args(args)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }
    let before = refs();

    let lookup_test = Command::new(std::env::current_exe().expect("locate this test binary"))
        .args([
            "--exact",
            "update_tag_lookup_picks_the_newest_release_and_skips_suffixed_tags",
            "--test-threads=1",
        ])
        .env("GIT_DIR", repository.join(".git"))
        .env("GIT_WORK_TREE", &repository)
        .env("GIT_INDEX_FILE", repository.join(".git/index"))
        .output()
        .expect("run the tag lookup test with GIT_DIR set");
    let report = String::from_utf8_lossy(&lookup_test.stdout);
    assert!(lookup_test.status.success(), "{lookup_test:?}");
    assert!(
        report.contains("running 1 test\n") && report.contains("test result: ok. 1 passed;"),
        "the guard must run the tag lookup test exactly once: {report}"
    );
    assert_eq!(refs(), before, "the tag lookup test wrote into GIT_DIR");
}

fn isolated(directory: &Path, program: &str) -> Command {
    let mut command = Command::new(program);
    // Git exports GIT_DIR to hooks, so an inherited one would send these writes to the developer's repository.
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(name);
        }
    }
    command
        .current_dir(directory)
        .env(
            "GIT_CEILING_DIRECTORIES",
            directory.parent().unwrap_or(directory),
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", directory.join("absent-global-config"))
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid");
    command
}

fn directory_is_empty(path: &std::path::Path) -> bool {
    fs::read_dir(path)
        .expect("read isolated directory")
        .next()
        .is_none()
}
