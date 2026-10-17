use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;
use support::TestEnvironment;
use toml_edit::DocumentMut;

#[path = "init/opencode.rs"]
mod opencode;

#[test]
fn fresh_init_is_private_empty_non_destructive_and_idempotent() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    initialize_git_repository(&environment.workspace);
    let protected = create_protected_workspace_files(&environment.workspace);

    let first = environment
        .command()
        .arg("init")
        .output()
        .expect("run magnesium init");
    assert_success(&first);

    let canonical_workspace = fs::canonicalize(&environment.workspace).unwrap();
    let control = canonical_workspace.join(support::CONTROL_DIR);
    for directory in ["", "index", "cache", "scratch", "backups"] {
        assert!(control.join(directory).is_dir(), "missing {directory}");
    }
    for file in [
        "config.toml",
        "INSTRUCTIONS.md",
        "state.sqlite",
        "magnesiumignore",
        "index/source-search.sqlite",
    ] {
        assert!(control.join(file).is_file(), "missing {file}");
    }
    let instructions = fs::read_to_string(control.join("INSTRUCTIONS.md")).unwrap();
    assert!(instructions.contains("Magnesium Workspace Instructions"));
    assert!(instructions.len() < 16 * 1024);
    assert_empty_learner_state(&control.join("state.sqlite"));
    assert_valid_empty_source_cache(&control.join("index/source-search.sqlite"));
    assert_workspace_files_unchanged(&protected);

    let exclude = canonical_workspace.join(".git/info/exclude");
    let exclude_after_first = fs::read(&exclude).expect("read Git private exclude");
    let control_line = format!("{}/", support::CONTROL_DIR);
    assert_eq!(
        exclude_after_first
            .split(|byte| *byte == b'\n')
            .filter(|line| *line == control_line.as_bytes())
            .count(),
        1
    );
    let config_after_first = fs::read(control.join("config.toml")).unwrap();
    let initialized_id = workspace_id(&control.join("config.toml"));
    assert!(initialized_id.starts_with("ws_"));
    assert_eq!(environment.unmanaged_data_entries(), Vec::<String>::new());

    let stdout = String::from_utf8(first.stdout).expect("init output is UTF-8");
    let reported_paths = [
        control.clone(),
        control.join("config.toml"),
        control.join("INSTRUCTIONS.md"),
        control.join("state.sqlite"),
        control.join("magnesiumignore"),
        control.join("index/source-search.sqlite"),
        exclude.clone(),
    ];
    for path in &reported_paths {
        assert!(
            stdout.contains(&path.display().to_string()),
            "output omitted {}",
            path.display()
        );
    }
    assert!(stdout.contains("tracking: enabled by default"));
    assert!(stdout.contains("learner state: empty"));

    #[cfg(unix)]
    assert_private_permissions(&control);

    let second = environment
        .command()
        .arg("init")
        .output()
        .expect("rerun magnesium init");
    assert_success(&second);
    assert_eq!(
        fs::read(control.join("config.toml")).unwrap(),
        config_after_first
    );
    assert_eq!(fs::read(&exclude).unwrap(), exclude_after_first);
    assert_eq!(environment.unmanaged_data_entries(), Vec::<String>::new());
    assert_eq!(workspace_id(&control.join("config.toml")), initialized_id);
    assert_empty_learner_state(&control.join("state.sqlite"));
    assert_workspace_files_unchanged(&protected);
}

#[test]
fn init_rejects_incompatible_source_cache_without_modifying_it() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    assert_success(&environment.command().arg("init").output().unwrap());
    let path = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("index/source-search.sqlite");
    let connection = Connection::open(&path).unwrap();
    connection.pragma_update(None, "user_version", 7).unwrap();
    drop(connection);
    let before = fs::read(&path).unwrap();
    let output = environment.command().arg("init").output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("run magnesium init"), "{error}");
    assert!(
        error.contains(&format!(
            "Do not move or delete {}/state.sqlite",
            support::CONTROL_DIR
        )),
        "{error}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn init_accepts_one_explicit_workspace_path() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let output = environment
        .command_in(&environment.root)
        .arg("init")
        .arg(&environment.workspace)
        .output()
        .expect("initialize explicit path");

    assert_success(&output);
    assert!(
        environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("config.toml")
            .is_file()
    );
    assert!(!environment.root.join(support::CONTROL_DIR).exists());
}

#[test]
fn invalid_existing_config_is_preserved_with_a_resumable_diagnostic() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let control = environment.workspace.join(support::CONTROL_DIR);
    fs::create_dir(&control).unwrap();
    let config = control.join("config.toml");
    let invalid = b"workspace_id = [not valid TOML\n";
    fs::write(&config, invalid).unwrap();
    let lesson = fs::read(environment.workspace.join("lesson.md")).unwrap();

    let output = environment
        .command()
        .arg("init")
        .output()
        .expect("run failing init");

    assert!(!output.status.success());
    assert_eq!(fs::read(&config).unwrap(), invalid);
    assert_eq!(
        fs::read(environment.workspace.join("lesson.md")).unwrap(),
        lesson
    );
    let error = String::from_utf8(output.stderr).expect("error is UTF-8");
    assert!(error.contains("invalid Magnesium config"));
    assert!(error.contains("rerun `magnesium init` to resume"));
    assert_eq!(environment.unmanaged_data_entries(), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn control_symlink_fails_before_touching_its_external_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let external = environment.root.join("external-control-target");
    fs::create_dir(&external).unwrap();
    fs::set_permissions(&external, fs::Permissions::from_mode(0o755)).unwrap();
    let sentinel = external.join("sentinel.bin");
    let bytes = b"external user state\0must stay unchanged";
    fs::write(&sentinel, bytes).unwrap();
    symlink(&external, environment.workspace.join(support::CONTROL_DIR)).unwrap();
    let permissions_before = fs::metadata(&external).unwrap().permissions().mode();

    let output = environment
        .command()
        .arg("init")
        .output()
        .expect("reject symlinked control directory");

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("is a symbolic link")
    );
    assert_eq!(fs::read(&sentinel).unwrap(), bytes);
    assert_eq!(
        fs::metadata(&external).unwrap().permissions().mode(),
        permissions_before
    );
    assert_eq!(fs::read_dir(&external).unwrap().count(), 1);
    assert_eq!(environment.unmanaged_data_entries(), Vec::<String>::new());
}

#[cfg(target_os = "linux")]
#[test]
fn nested_git_exclusion_preserves_non_utf8_path_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let repository = environment.root.join("repository");
    fs::create_dir(&repository).unwrap();
    initialize_git_repository(&repository);
    let component = OsString::from_vec(b"nested-\xff".to_vec());
    let workspace = repository.join(&component);
    fs::create_dir(&workspace).unwrap();

    let output = environment
        .command_in(&workspace)
        .arg("init")
        .output()
        .expect("initialize non-UTF-8 nested workspace");

    assert_success(&output);
    let exclude = fs::read(repository.join(".git/info/exclude")).unwrap();
    let expected = [
        b"/nested-\xff/".as_slice(),
        support::CONTROL_DIR.as_bytes(),
        b"/",
    ]
    .concat();
    assert!(
        exclude
            .windows(expected.len())
            .any(|window| window == expected)
    );
}

fn initialize_git_repository(workspace: &Path) {
    let status = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(workspace)
        .status()
        .expect("run git init");
    assert!(status.success());
}

fn create_protected_workspace_files(workspace: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let files = [
        ("AGENTS.md", b"user-owned agents\n".as_slice()),
        (".gitignore", b"target/\n".as_slice()),
        (
            ".codex/config.toml",
            b"model = \"user-choice\"\n".as_slice(),
        ),
        (".codex/workflows/review.md", b"user workflow\n".as_slice()),
        (".codex/permissions.toml", b"allow = []\n".as_slice()),
        (".codex/plugins/local.toml", b"enabled = true\n".as_slice()),
        (".github/workflows/ci.yml", b"name: user-ci\n".as_slice()),
    ];
    let mut snapshots = BTreeMap::new();
    for (relative, contents) in files {
        let path = workspace.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, contents).unwrap();
        snapshots.insert(path, contents.to_vec());
    }
    snapshots
}

fn assert_workspace_files_unchanged(snapshots: &BTreeMap<PathBuf, Vec<u8>>) {
    for (path, expected) in snapshots {
        assert_eq!(
            &fs::read(path).unwrap(),
            expected,
            "changed {}",
            path.display()
        );
    }
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn workspace_id(config: &Path) -> String {
    fs::read_to_string(config)
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap()["workspace_id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn assert_empty_learner_state(path: &Path) {
    let connection = Connection::open(path).unwrap();
    for table in [
        "learner_memories",
        "observations",
        "learning_events",
        "learning_sessions",
    ] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "{table} was not empty");
    }
    let family: String = connection
        .query_row("SELECT storage_family FROM workspace_meta", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(family, "learner-model/v2");
}

fn assert_valid_empty_source_cache(path: &Path) {
    let connection = Connection::open(path).unwrap();
    let integrity: String = connection
        .query_row("PRAGMA quick_check(1)", [], |row| row.get(0))
        .unwrap();
    let generation: i64 = connection
        .query_row("SELECT generation FROM source_index_metadata", [], |row| {
            row.get(0)
        })
        .unwrap();
    let manifests: i64 = connection
        .query_row("SELECT count(*) FROM source_manifest", [], |row| row.get(0))
        .unwrap();
    let candidates: i64 = connection
        .query_row("SELECT count(*) FROM source_candidates", [], |row| {
            row.get(0)
        })
        .unwrap();
    let identity: (i64, i64) = (
        connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .unwrap(),
        connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap(),
    );
    assert_eq!(integrity, "ok");
    assert_eq!((generation, manifests, candidates), (0, 0, 0));
    // ASCII "MGSI" (Magnesium source index) and the cache schema version.
    assert_eq!(identity, (0x4d47_5349, 10));
}

#[cfg(unix)]
fn assert_private_permissions(control: &Path) {
    use std::os::unix::fs::PermissionsExt;

    for directory in ["", "index", "cache", "scratch", "backups"] {
        assert_eq!(
            fs::metadata(control.join(directory))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    for file in [
        "config.toml",
        "INSTRUCTIONS.md",
        "state.sqlite",
        "magnesiumignore",
        "index/source-search.sqlite",
    ] {
        assert_eq!(
            fs::metadata(control.join(file))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn init_refreshes_instructions_idempotently_without_modifying_other_files() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let init1 = environment
        .command()
        .arg("init")
        .output()
        .expect("run init");
    assert_success(&init1);
    let instructions_path = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("INSTRUCTIONS.md");
    assert!(instructions_path.is_file());

    // Tamper with instructions to simulate an outdated version
    fs::write(&instructions_path, b"outdated instructions\n").unwrap();

    let init2 = environment
        .command()
        .arg("init")
        .output()
        .expect("rerun init");
    assert_success(&init2);
    let refreshed = fs::read_to_string(&instructions_path).unwrap();
    assert!(refreshed.contains("Magnesium Workspace Instructions"));
    assert!(refreshed.len() < 16 * 1024);
}

#[test]
fn init_with_adapter_agents_md_creates_and_updates_bounded_block_preserves_external_bytes() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let agents_md = environment.workspace.join("AGENTS.md");

    // Case 1: AGENTS.md does not exist -> created with managed block
    let output1 = environment
        .command()
        .args(["init", "--adapter", "agents-md"])
        .output()
        .expect("run init --adapter agents-md");
    assert_success(&output1);
    let stdout1 = String::from_utf8(output1.stdout).expect("utf8");
    assert!(stdout1.contains("adapter: AGENTS.md bootstrap configured"));
    let content1 = fs::read_to_string(&agents_md).expect("read AGENTS.md");
    assert!(content1.contains("<!-- magnesium:begin -->"));
    assert!(content1.contains("<!-- magnesium:end -->"));
    assert!(content1.contains(&format!("{}/INSTRUCTIONS.md", support::CONTROL_DIR)));

    // Case 2: AGENTS.md has other user content -> managed block is updated without modifying user content
    let custom_prefix = "# User Rules\nDo not break production.\n\n";
    let custom_suffix = "\n## Other Rules\nKeep tests green.\n";
    let combined = format!("{custom_prefix}{content1}{custom_suffix}");
    fs::write(&agents_md, &combined).expect("write user content");

    let output2 = environment
        .command()
        .args(["init", "--adapter", "agents-md"])
        .output()
        .expect("rerun init --adapter agents-md");
    assert_success(&output2);
    let content2 = fs::read_to_string(&agents_md).expect("read updated AGENTS.md");
    assert!(content2.starts_with(custom_prefix));
    assert!(content2.ends_with(custom_suffix));
    assert!(content2.contains("<!-- magnesium:begin -->"));
    assert!(content2.contains("<!-- magnesium:end -->"));

    // Case 3: AGENTS.md exists with user content and NO managed block -> block is appended
    let no_block = "# Plain Agents\nSome instructions.\n";
    fs::write(&agents_md, no_block).expect("write plain content");
    let output3 = environment
        .command()
        .args(["init", "--adapter", "agents-md"])
        .output()
        .expect("run init with plain AGENTS.md");
    assert_success(&output3);
    let content3 = fs::read_to_string(&agents_md).expect("read appended AGENTS.md");
    assert!(content3.starts_with(no_block));
    assert!(content3.contains("<!-- magnesium:begin -->"));
    assert!(content3.contains("<!-- magnesium:end -->"));
}

#[test]
fn init_without_adapter_does_not_touch_agents_md() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let agents_md = environment.workspace.join("AGENTS.md");
    let initial_content = "# Preserved User Rules\nUntouched content.\n";
    fs::write(&agents_md, initial_content).expect("write agents.md");

    let output = environment
        .command()
        .arg("init")
        .output()
        .expect("run init without adapter");
    assert_success(&output);
    assert_eq!(
        fs::read_to_string(&agents_md).expect("read agents.md"),
        initial_content
    );
}

#[test]
fn init_bundled_marketplace_materialization_and_status() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let output = environment
        .command()
        .arg("init")
        .output()
        .expect("run init");
    assert_success(&output);

    let marketplace = environment.data_dir().join("marketplace");
    assert!(
        marketplace
            .join(".agents/plugins/marketplace.json")
            .is_file()
    );
    assert!(
        marketplace
            .join("codex-plugin/.codex-plugin/plugin.json")
            .is_file()
    );
    assert!(marketplace.join("codex-plugin/.mcp.json").is_file());
    assert!(marketplace.join("codex-plugin/hooks/hooks.json").is_file());
    assert!(
        marketplace
            .join("codex-plugin/skills/learn/SKILL.md")
            .is_file()
    );

    let skill_path = marketplace.join("codex-plugin/skills/learn/SKILL.md");
    let skill = fs::read_to_string(&skill_path).unwrap();
    let source_skill = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("codex-plugin/skills/learn/SKILL.md"),
    )
    .unwrap();
    assert!(skill.contains("INSTRUCTIONS.md"));
    assert_eq!(
        skill, source_skill,
        "init must materialize the embedded skill verbatim"
    );
    let (_, after_opening) = skill.split_once("---\n").unwrap();
    let (frontmatter, body) = after_opening.split_once("\n---\n").unwrap();
    assert!(frontmatter.contains("explicit learning, teaching, practice, or review request"));
    assert!(body.contains("learner-model/v2"));
    assert!(
        body.contains("Repository work")
            || body.contains("Ordinary implementation work remains neutral")
    );
    assert!(body.contains("context(action:\"instructions\",host:\"codex\""));
    assert!(body.contains("teaching-policy/v2"));
}

#[test]
fn init_with_invalid_adapter_fails_zero_delta() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let output = environment
        .command()
        .args(["init", "--adapter", "unsupported-adapter-name"])
        .output()
        .expect("run init with invalid adapter");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unsupported adapter type `unsupported-adapter-name`"));

    // Zero-delta invariant: the control directory must not exist.
    let control = environment.workspace.join(support::CONTROL_DIR);
    assert!(
        !control.exists(),
        "control directory was created despite invalid adapter"
    );
}

#[test]
fn init_agents_md_preserves_permissions_and_newlines() {
    let environment = TestEnvironment::from_fixture("basic").expect("create test environment");
    let agents_md = environment.workspace.join("AGENTS.md");
    let crlf_content = "# Project Custom Rules\r\n\r\nLine 2 custom rule.\r\n";
    fs::write(&agents_md, crlf_content).expect("write initial crlf AGENTS.md");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&agents_md, fs::Permissions::from_mode(0o644)).unwrap();
        fs::set_permissions(&environment.workspace, fs::Permissions::from_mode(0o755)).unwrap();
    }

    let output = environment
        .command()
        .args(["init", "--adapter", "agents-md"])
        .output()
        .expect("run init with agents-md adapter");
    assert_success(&output);

    let updated_content = fs::read_to_string(&agents_md).expect("read updated AGENTS.md");
    assert!(updated_content.contains("# Project Custom Rules"));
    assert!(updated_content.contains("<!-- magnesium:begin -->"));
    assert!(
        updated_content.contains("\r\n"),
        "CRLF newlines were not preserved"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let file_mode = fs::metadata(&agents_md).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            file_mode, 0o644,
            "AGENTS.md permissions were changed by init"
        );
        let dir_mode = fs::metadata(&environment.workspace)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            dir_mode, 0o755,
            "Workspace directory permissions were changed by init"
        );
    }
}

#[test]
fn init_json_is_machine_readable_and_idempotent() {
    let environment = TestEnvironment::from_fixture("basic").expect("create environment");
    let first = {
        let output = environment
            .command()
            .args(["init", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    assert_eq!(first["ok"], true);
    assert_eq!(first["learner_state"], "empty");
    assert!(
        first["root"].as_str().is_some_and(|root| !root.is_empty()),
        "report carries the workspace root: {first}"
    );
    fs::write(environment.workspace.join("keep.md"), "user file").expect("write user file");
    let second = {
        let output = environment
            .command()
            .args(["init", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    assert_eq!(second["ok"], true);
    assert_eq!(second["learner_state"], "preserved");
    assert_eq!(
        fs::read_to_string(environment.workspace.join("keep.md")).expect("read user file"),
        "user file",
        "explicit re-init preserves user files"
    );
}
