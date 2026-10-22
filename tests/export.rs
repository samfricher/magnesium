//! Export the actual v2 writer's records through the installed CLI boundary.
use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
use support::mcp::McpClient;
use support::{TestEnvironment, v2};

fn written_workspace() -> (TestEnvironment, String) {
    let env = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&env);
    v2::start(&env, "codex", "export-thread");
    let captured = v2::tool(
        &env,
        "codex",
        "export-thread",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"capture","reported_events":[{"external_event_key":"original-answer","speaker":"learner","kind":"message","text":"I used the quadratic formula correctly but confused b with the constant.","metadata":{}}]}),
    );
    let event = captured["captured_event_ids"][0].as_str().unwrap();
    fs::write(
        env.workspace.join("allowed.txt"),
        "A coefficient multiplies its variable.",
    )
    .unwrap();
    fs::write(
        env.workspace.join("ignored.txt"),
        "PRIVATE_IGNORED_SENTINEL",
    )
    .unwrap();
    fs::write(
        env.workspace
            .join(support::CONTROL_DIR)
            .join("magnesiumignore"),
        "ignored.txt\n",
    )
    .unwrap();
    fs::write(env.workspace.join(".env"), "PRIVATE_ENV_SENTINEL").unwrap();
    let memory = v2::tool(
        &env,
        "codex",
        "export-thread",
        "learning_step",
        json!({"schema":"learner-model/v2","action":"update","request_id":"describe","turn_ref":event,"observations":[{"client_ref":"o","kind":"learner_question","description":"Substitution succeeds; coefficient identification needs clarification.","events":[{"event_id":event,"role":"learner_response","excerpts":[]}],"source_refs":[{"path":"allowed.txt","available_form":"original"},{"path":"ignored.txt","available_form":"original"},{"path":".env","available_form":"original"},{"path":"missing.txt","available_form":"unavailable"}]}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":"Coefficient roles","scope":"Quadratics","summary":"Preserve formula application strength.","state_text":"The learner has not yet distinguished b from the constant c.","current_basis":[{"new_observation":"o"}]},"reason":"Original learner explanation","change_basis":[{"new_observation":"o"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]}),
    );
    let id = memory["references"]["m"].as_str().unwrap().to_owned();
    (env, id)
}
fn export(env: &TestEnvironment, destination: &Path, extra: &[&str]) -> std::process::Output {
    env.command()
        .arg("export")
        .arg(destination)
        .args(extra)
        .output()
        .unwrap()
}
fn successful(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn workspace_relative_source(workspace: &Path, source: &Path) -> String {
    // macOS temp paths may name the same directory through /var and
    // /private/var. Export returns its canonical destination, so compare both
    // existing paths in that same representation before making a tool path.
    let workspace = fs::canonicalize(workspace).unwrap();
    fs::canonicalize(source)
        .unwrap()
        .strip_prefix(workspace)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn search_after_preparation(
    env: &TestEnvironment,
    query: &str,
    required: Option<(&str, &[u8])>,
    assert_snapshot: impl Fn(&Value),
) -> Value {
    if let Some((path, bytes)) = required {
        support::sources::prepare_text_fixture_separately(env, path, bytes);
    }
    // This semantic/privacy oracle is one original request. In particular,
    // exclusion and empty negative results are never retried.
    let response = v2::tool(
        env,
        "codex",
        "export-thread",
        "search_sources",
        json!({"query":query, "limit":50}),
    );
    assert_snapshot(&response);
    if let Some((path, bytes)) = required {
        let hash = support::sources::fixture_hash(bytes);
        let hit = response["hits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|hit| hit["path"] == path)
            .unwrap_or_else(|| panic!("prepared semantic search missing {path}: {response}"));
        assert_eq!(hit["content_sha256"], hash, "{response}");
        assert_eq!(hit["anchor_kind"], "line", "{response}");
    }
    response
}

fn zip_entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut entries = BTreeMap::new();
    let mut offset = 0;
    while bytes.get(offset..offset + 4) == Some(b"PK\x03\x04") {
        let u16_at = |i| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap()) as usize;
        let size = u32::from_le_bytes(bytes[offset + 18..offset + 22].try_into().unwrap()) as usize;
        assert_eq!(u16_at(offset + 8), 0, "stored ZIP entry");
        let names = u16_at(offset + 26);
        let extra = u16_at(offset + 28);
        let name = std::str::from_utf8(&bytes[offset + 30..offset + 30 + names])
            .unwrap()
            .to_owned();
        let start = offset + 30 + names + extra;
        assert!(
            entries
                .insert(name, bytes[start..start + size].to_vec())
                .is_none()
        );
        offset = start + size;
    }
    assert_eq!(
        bytes.get(offset..offset + 4),
        Some(b"PK\x01\x02".as_slice())
    );
    entries
}
#[test]
fn cli_export_preserves_original_work_memory_history_and_exact_context_packets() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, id) = written_workspace();
    let packet = v2::tool(
        &env,
        "codex",
        "export-thread",
        "context",
        json!({"schema":"learner-model/v2","action":"prepare"}),
    );
    let destination = env.root.join("export");
    successful(export(&env, &destination, &[]));
    let data = read_json(&destination.join("learner.json"));
    assert_eq!(data["learner_memories"][0]["memory_id"], id);
    assert_eq!(data["memory_changes"][0]["revision"], 1);
    assert_eq!(data["learning_events"][0]["origin"], "model_reported");
    assert!(
        data["learning_events"][0]["text"]
            .as_str()
            .unwrap()
            .contains("confused b")
    );
    let receipt: Value = serde_json::from_str(
        data["context_receipts"][0]["response_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(receipt, packet);
    assert!(
        fs::read_to_string(destination.join("README.md"))
            .unwrap()
            .contains("formula application strength")
    );
    assert!(!destination.join("sources").exists());
    // learner.json holds exactly the workspace learner tables, nothing scoped wider.
    assert_eq!(
        data.as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from([
            "context_receipts",
            "host_bindings",
            "learner_memories",
            "learning_events",
            "learning_sessions",
            "memory_changes",
            "memory_observations",
            "memory_topics",
            "observation_events",
            "observations",
            "operation_receipts",
            "review_items",
            "workspace_meta",
        ])
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(destination.join("learner.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
}
#[test]
fn cli_source_copies_are_explicit_scoped_and_honestly_report_missing_material() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, _) = written_workspace();
    let dest = env.root.join("copies");
    successful(export(&env, &dest, &["--include-source-copies"]));
    assert_eq!(
        fs::read(dest.join("sources/allowed.txt")).unwrap(),
        fs::read(env.workspace.join("allowed.txt")).unwrap()
    );
    assert!(!dest.join("sources/ignored.txt").exists());
    assert!(!dest.join("sources/.env").exists());
    let manifest = read_json(&dest.join("manifest.json"));
    let coverage = manifest["source_coverage"].as_array().unwrap();
    assert!(
        coverage
            .iter()
            .any(|entry| entry["path"] == "missing.txt" && entry["status"] == "unavailable")
    );
    assert!(
        coverage
            .iter()
            .any(|entry| entry["path"] == "ignored.txt" && entry["reason"] == "source_excluded")
    );
}
#[test]
fn cli_zip_exports_open_without_magnesium_and_overwrite_retains_previous_artifact() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, id) = written_workspace();
    let dest = env.root.join("export.zip");
    successful(export(&env, &dest, &["--format", "zip"]));
    let bytes = fs::read(&dest).unwrap();
    let entries = zip_entries(&bytes);
    let data: Value = serde_json::from_slice(&entries["learner.json"]).unwrap();
    assert_eq!(data["learner_memories"][0]["memory_id"], id);
    assert!(!export(&env, &dest, &["--format", "zip"]).status.success());
    assert_eq!(fs::read(&dest).unwrap(), bytes);
    let output = successful(export(&env, &dest, &["--format", "zip", "--overwrite"]));
    let backup = Path::new(output["previous_export_backup"].as_str().unwrap());
    assert_eq!(fs::read(backup).unwrap(), bytes);
    assert!(dest.exists());
}
#[test]
fn cli_overwrite_preserves_user_additions_and_refuses_unrelated_directories() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, _) = written_workspace();
    let dest = env.root.join("export");
    successful(export(&env, &dest, &[]));
    fs::write(dest.join("my-reflection.md"), "Do not lose my notes").unwrap();
    let output = successful(export(&env, &dest, &["--overwrite"]));
    let backup = Path::new(output["previous_export_backup"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(backup.join("my-reflection.md")).unwrap(),
        "Do not lose my notes"
    );
    let arbitrary = env.root.join("my-work");
    fs::create_dir(&arbitrary).unwrap();
    fs::write(arbitrary.join("keep.txt"), "KEEP").unwrap();
    assert!(!export(&env, &arbitrary, &["--overwrite"]).status.success());
    assert_eq!(
        fs::read_to_string(arbitrary.join("keep.txt")).unwrap(),
        "KEEP"
    );
    assert!(
        !export(&env, &env.workspace, &["--overwrite"])
            .status
            .success()
    );
    assert!(!export(&env, &env.root, &["--overwrite"]).status.success());
    assert!(
        !export(
            &env,
            &env.workspace.join(support::CONTROL_DIR).join("export"),
            &[]
        )
        .status
        .success()
    );
}
#[cfg(unix)]
#[test]
fn cli_export_rejects_destination_symlinks_without_touching_target() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, _) = written_workspace();
    let target = env.root.join("target.txt");
    fs::write(&target, "KEEP").unwrap();
    let link = env.root.join("linked-export");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(!export(&env, &link, &["--overwrite"]).status.success());
    assert_eq!(fs::read_to_string(target).unwrap(), "KEEP");
}

#[test]
fn cli_directory_exports_remain_private_after_rename_copy_and_overwrite() {
    let _source_capacity = support::sources::source_test_guard();
    fn copy_directory(from: &Path, to: &Path) {
        fs::create_dir(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy_directory(&entry.path(), &to.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    let (env, _) = written_workspace();
    let course = b"coefficient subject material belongs in ordinary retrieval\n";
    fs::write(env.workspace.join("course.md"), course).unwrap();
    let destination = env.workspace.join("review");
    successful(export(&env, &destination, &[]));
    assert!(destination.join(".magnesium-export").is_file());
    let data = fs::read(destination.join("learner.json")).unwrap();
    assert!(
        !read_json(&destination.join("learner.json"))["memory_changes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let copied = env.workspace.join("copied");
    copy_directory(&destination, &copied);
    let renamed = env.workspace.join("renamed");
    fs::rename(&destination, &renamed).unwrap();
    fs::write(renamed.join("personal.md"), "coefficient personal addition").unwrap();
    // A valid export whose boundary marker was deleted must become safe
    // again when preserved as the previous-export backup.
    fs::remove_file(renamed.join(".magnesium-export")).unwrap();
    let output = successful(export(&env, &renamed, &["--overwrite"]));
    let backup = Path::new(output["previous_export_backup"].as_str().unwrap());
    for directory in [&copied, &renamed, backup] {
        assert!(directory.join(".magnesium-export").is_file());
        for filename in ["learner.json", "README.md", "manifest.json"] {
            let relative = workspace_relative_source(&env.workspace, &directory.join(filename));
            let inspected = v2::tool(
                &env,
                "codex",
                "export-thread",
                "sources",
                json!({"action":"inspect", "path":relative}),
            );
            assert_eq!(inspected["coverage"]["status"], "excluded", "{inspected}");
            assert!(
                inspected.get("original").is_none() && inspected.get("excerpt").is_none(),
                "{inspected}"
            );
        }
    }
    let search = search_after_preparation(
        &env,
        "coefficient",
        Some(("course.md", course.as_slice())),
        |response| {
            assert!(
                response["hits"].as_array().unwrap().iter().all(
                    |hit| ["course.md", "allowed.txt"].contains(&hit["path"].as_str().unwrap())
                ),
                "{response}"
            );
        },
    );
    let hits = search["hits"].as_array().unwrap();
    assert!(
        hits.iter().any(|hit| hit["path"] == "course.md"),
        "{search}"
    );
    assert!(
        hits.iter()
            .all(|hit| ["course.md", "allowed.txt"].contains(&hit["path"].as_str().unwrap())),
        "{search}"
    );
    assert_eq!(fs::read(copied.join("learner.json")).unwrap(), data);
    assert_eq!(fs::read(backup.join("learner.json")).unwrap(), data);
    assert_eq!(
        fs::read_to_string(backup.join("personal.md")).unwrap(),
        "coefficient personal addition"
    );
    assert_eq!(fs::read(env.workspace.join("course.md")).unwrap(), course);
}

#[test]
fn cli_export_marker_revokes_direct_reads_of_previously_indexed_text() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, _) = written_workspace();
    let directory = env.workspace.join("review");
    fs::create_dir(&directory).unwrap();
    let private = b"isolationfixture private historical answer\n";
    fs::write(directory.join("learner.json"), private).unwrap();
    let before = search_after_preparation(
        &env,
        "isolationfixture",
        Some(("review/learner.json", private.as_slice())),
        |_| {},
    );
    let hit = before["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["path"] == "review/learner.json")
        .expect("actual private source hit before adding marker");
    assert_eq!(hit["path"], "review/learner.json", "{before}");
    assert_eq!(
        hit["content_sha256"].as_str().unwrap().len(),
        64,
        "{before}"
    );
    assert_eq!(hit["anchor_kind"], "line", "{before}");
    let start = hit["anchor_start"].as_u64().unwrap();
    let end = hit["anchor_end"].as_u64().unwrap();
    assert!(start >= 1 && end >= start, "{before}");
    fs::write(directory.join(".magnesium-export"), "magnesium-export-v2\n").unwrap();
    for request in [
        json!({"action":"inspect", "path":"review/learner.json"}),
        json!({"action":"inspect", "path":hit["path"], "content_sha256":hit["content_sha256"], "anchor_kind":hit["anchor_kind"], "anchor_start":hit["anchor_start"], "anchor_end":hit["anchor_end"]}),
    ] {
        let inspected = v2::tool(&env, "codex", "export-thread", "sources", request);
        assert_eq!(inspected["coverage"]["status"], "excluded", "{inspected}");
        assert!(
            inspected.get("original").is_none() && inspected.get("excerpt").is_none(),
            "{inspected}"
        );
    }
    let after = search_after_preparation(&env, "isolationfixture", None, |response| {
        assert!(
            response["hits"].as_array().unwrap().is_empty(),
            "{response}"
        );
    });
    assert!(after["hits"].as_array().unwrap().is_empty(), "{after}");
    assert_eq!(fs::read(directory.join("learner.json")).unwrap(), private);
}

#[test]
fn cli_export_marker_at_initialized_workspace_root_blocks_sources() {
    let _source_capacity = support::sources::source_test_guard();
    let (mut env, _) = written_workspace();
    let copied_root = env.root.join("copied-root");
    successful(export(&env, &copied_root, &[]));
    let data = fs::read(copied_root.join("learner.json")).unwrap();
    env.workspace = copied_root;
    v2::init(&env);
    let search = search_after_preparation(&env, "coefficient", None, |response| {
        assert!(
            response["hits"].as_array().unwrap().is_empty(),
            "{response}"
        );
    });
    assert!(search["hits"].as_array().unwrap().is_empty(), "{search}");
    for path in ["learner.json", "README.md"] {
        let inspected = v2::tool(
            &env,
            "codex",
            "export-thread",
            "sources",
            json!({"action":"inspect", "path":path}),
        );
        assert_eq!(inspected["coverage"]["status"], "excluded", "{inspected}");
        assert!(
            inspected.get("original").is_none() && inspected.get("excerpt").is_none(),
            "{inspected}"
        );
    }
    let index = rusqlite::Connection::open(
        env.workspace
            .join(support::CONTROL_DIR)
            .join("index/source-search.sqlite"),
    )
    .unwrap();
    assert_eq!(
        index
            .query_row("SELECT count(*) FROM source_candidates", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(fs::read(env.workspace.join("learner.json")).unwrap(), data);
}

#[test]
fn cli_overwrite_never_replaces_or_follows_a_hostile_export_marker() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, _) = written_workspace();
    let destination = env.workspace.join("review");
    successful(export(&env, &destination, &[]));
    let data = fs::read(destination.join("learner.json")).unwrap();
    let marker = destination.join(".magnesium-export");
    fs::remove_file(&marker).unwrap();
    fs::create_dir(&marker).unwrap();
    assert!(
        !export(&env, &destination, &["--overwrite"])
            .status
            .success()
    );
    assert!(marker.is_dir());
    assert_eq!(fs::read(destination.join("learner.json")).unwrap(), data);
    #[cfg(unix)]
    {
        fs::remove_dir(&marker).unwrap();
        let source = env.workspace.join("source.md");
        fs::write(&source, "ordinary source content").unwrap();
        std::os::unix::fs::symlink(&source, &marker).unwrap();
        assert!(
            !export(&env, &destination, &["--overwrite"])
                .status
                .success()
        );
        assert!(
            fs::symlink_metadata(&marker)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(source).unwrap(),
            "ordinary source content"
        );
        assert_eq!(fs::read(destination.join("learner.json")).unwrap(), data);
    }
}

fn valid_pdf(text: &str) -> Vec<u8> {
    pdf_with_compression(text, false)
}

fn pdf_with_compression(text: &str, compressed: bool) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let font_id = document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = document.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
            Operation::new("Td", vec![72.into(), 720.into()]),
            Operation::new("Tj", vec![Object::string_literal(text)]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = document.add_object(Stream::new(
        dictionary! {},
        content.encode().expect("encode PDF text stream"),
    ));
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    if compressed {
        document.compress();
    }
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("serialize PDF fixture");
    bytes
}

fn prepare_pdf_source(
    environment: &TestEnvironment,
    path: &str,
    expected_bytes: &[u8],
) -> (String, Vec<Value>) {
    let hash = Sha256::digest(expected_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut client = McpClient::spawn(environment, true);
    client.initialize();
    let preparation =
        support::sources::prepare_pdf_fixture(environment, &mut client, 10, path, &hash);
    client.close_cleanly();
    (hash, preparation)
}

fn record_pdf_reference(environment: &TestEnvironment, path: &str, hash: &str) {
    let captured = v2::tool(
        environment,
        "codex",
        "export-thread",
        "learning_step",
        json!({
            "schema":"learner-model/v2", "action":"update", "request_id":"pdf-question",
            "reported_events":[{"external_event_key":"pdf-question","speaker":"learner","kind":"message","text":"Explain the WAL end mark."}]
        }),
    );
    let event = captured["captured_event_ids"][0].as_str().unwrap();
    let written = v2::tool(
        environment,
        "codex",
        "export-thread",
        "learning_step",
        json!({
            "schema":"learner-model/v2", "action":"update", "request_id":"pdf-reference", "turn_ref":event,
            "observations":[{"client_ref":"pdf-observation","kind":"learner_question","description":"Asks about the cited original PDF.",
            "events":[{"event_id":event,"role":"learner_response"}],
            "source_refs":[{"path":path,"hash":hash,"available_form":"original"}]}],
            "event_dispositions":[{"event_id":event,"outcome":"processed"}]
        }),
    );
    assert!(
        written["references"]["pdf-observation"].is_string(),
        "{written}"
    );
}

fn exact_pdf_filenames() -> Vec<String> {
    let basenames = vec![
        "ordinary".to_owned(),
        "lesson draft".to_owned(),
        "learner's draft".to_owned(),
        "lesson(draft)".to_owned(),
        "données/étude α".to_owned(),
        (0..33)
            .map(|word| format!("w{word}"))
            .collect::<Vec<_>>()
            .join(" "),
    ];
    let basenames = if cfg!(unix) {
        basenames
            .into_iter()
            .chain(["lesson\"draft".to_owned(), "lesson*draft".to_owned()])
            .collect()
    } else {
        basenames
    };
    basenames
        .into_iter()
        .map(|basename| format!("{basename}.pdf"))
        .collect()
}

fn export_canonical_snapshot(environment: &TestEnvironment) -> support::snapshot::DatabaseSnapshot {
    support::snapshot::DatabaseSnapshot::capture(
        &environment
            .workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite"),
        &[
            "learning_sessions",
            "host_bindings",
            "learning_events",
            "observations",
            "learner_memories",
            "memory_changes",
            "operation_receipts",
            "review_items",
            "context_receipts",
        ],
    )
    .unwrap()
}

fn export_exact_pdf_copy(
    environment: &TestEnvironment,
    relative: &str,
    hash: &str,
    destination_name: &str,
) -> std::path::PathBuf {
    for attempt in 0..3 {
        let destination = environment
            .root
            .join(format!("{destination_name}-{attempt}"));
        successful(export(
            environment,
            &destination,
            &["--include-source-copies"],
        ));
        let manifest = read_json(&destination.join("manifest.json"));
        let coverage = manifest["source_coverage"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == relative)
            .unwrap();
        assert_eq!(coverage["requested_hash"], hash, "{coverage}");
        if coverage["status"] == "copied_current" {
            return destination;
        }
        assert!(
            attempt < 2
                && coverage["status"] == "unavailable"
                && matches!(
                    coverage["reason"].as_str(),
                    Some(
                        "source_preparation_deferred_budget"
                            | "source_preparation_deferred_concurrent_update"
                    )
                ),
            "exact PDF copy did not complete for {relative}: {coverage}"
        );
        assert!(
            !destination.join("sources").join(relative).exists(),
            "{coverage}"
        );
    }
    unreachable!()
}

#[test]
fn cli_exact_pdf_filename_matrix_copies_current_bytes_without_filename_search() {
    let _source_capacity = support::sources::source_test_guard();
    for relative in exact_pdf_filenames() {
        for warm in [false, true] {
            let (env, _) = written_workspace();
            let pdf = valid_pdf("Alpha WAL end mark payload");
            let target = env.workspace.join(&relative);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::write(&target, &pdf).unwrap();
            let hash = support::sources::fixture_hash(&pdf);
            if warm {
                let mut client = McpClient::spawn(&env, true);
                client.initialize();
                for attempt in 0..3 {
                    let response = client.call_value(
                        100 + attempt,
                        "search_sources",
                        json!({"query":"WAL end mark", "path_prefix":relative, "limit":20}),
                    );
                    if response["hits"].as_array().unwrap().iter().any(|hit| {
                        hit["path"] == relative
                            && hit["content_sha256"] == hash
                            && hit["authority"] == "original_source"
                    }) {
                        break;
                    }
                    assert!(
                        attempt < 2
                            && matches!(
                                response["reconciliation"].as_str(),
                                Some("deferred_budget" | "deferred_concurrent_update")
                            ),
                        "content query did not prepare {relative}: {response}"
                    );
                }
                client.close_cleanly();
            }
            record_pdf_reference(&env, &relative, &hash);
            let before = export_canonical_snapshot(&env);
            let references = env.root.join("references-only");
            successful(export(&env, &references, &[]));
            assert!(
                !references.join("sources").exists(),
                "{relative} warm={warm}"
            );
            let destination = export_exact_pdf_copy(&env, &relative, &hash, "source-copies");
            let copied = destination.join("sources").join(&relative);
            let manifest = read_json(&destination.join("manifest.json"));
            let coverage = manifest["source_coverage"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["path"] == relative)
                .unwrap();
            assert_eq!(
                coverage["status"], "copied_current",
                "{relative} warm={warm}: {coverage}"
            );
            assert_eq!(coverage["requested_hash"], hash, "{coverage}");
            assert_eq!(fs::read(&copied).unwrap(), pdf, "{relative} warm={warm}");
            assert_eq!(
                support::sources::fixture_hash(fs::read(&copied).unwrap()),
                hash,
                "{relative} warm={warm}"
            );
            assert_eq!(
                before,
                export_canonical_snapshot(&env),
                "{relative} warm={warm}"
            );
        }
    }
}

#[test]
fn cli_exact_pdf_copy_refuses_wrong_reference_hash() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, _) = written_workspace();
    let relative = "learner's (étude).pdf";
    let pdf = valid_pdf("Alpha WAL end mark payload");
    fs::write(env.workspace.join(relative), &pdf).unwrap();
    let reference_hash = "0".repeat(64);
    record_pdf_reference(&env, relative, &reference_hash);
    let before = export_canonical_snapshot(&env);
    let refused = env.root.join("refused-copy");
    successful(export(&env, &refused, &["--include-source-copies"]));
    assert!(!refused.join("sources").join(relative).exists());
    let manifest = read_json(&refused.join("manifest.json"));
    let coverage = manifest["source_coverage"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == relative)
        .unwrap();
    assert_eq!(coverage["status"], "unavailable", "{coverage}");
    assert_eq!(coverage["reason"], "source_changed", "{coverage}");
    assert_eq!(coverage["requested_hash"], reference_hash, "{coverage}");
    assert_eq!(before, export_canonical_snapshot(&env));
}

fn prepare_sensitive_pdf_source(environment: &TestEnvironment, path: &str, hash: &str) {
    use rusqlite::{OpenFlags, OptionalExtension};
    use std::time::{Duration, Instant};

    #[derive(serde::Serialize)]
    struct ManifestState {
        source_kind: String,
        index_status: String,
        sensitivity_class: Option<String>,
        content_sha256: Option<String>,
        last_error: Option<String>,
    }

    let started = Instant::now();
    let mut client = McpClient::spawn(environment, true);
    client.initialize();
    let mut history = Vec::new();
    for attempt in 0..32 {
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "sensitive fixture preparation deadline: {history:?}"
        );
        let response = client.call_value(
            1000 + attempt,
            "search_sources",
            json!({"query":path, "path_prefix":path, "limit":50}),
        );
        let index = rusqlite::Connection::open_with_flags(
            environment
                .workspace
                .join(support::CONTROL_DIR)
                .join("index/source-search.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .unwrap();
        let state = index
            .query_row(
                "SELECT source_kind, index_status, sensitivity_class, content_sha256, last_error
                 FROM source_manifest WHERE display_path = ?1",
                [path],
                |row| {
                    Ok(ManifestState {
                        source_kind: row.get(0)?,
                        index_status: row.get(1)?,
                        sensitivity_class: row.get(2)?,
                        content_sha256: row.get(3)?,
                        last_error: row.get(4)?,
                    })
                },
            )
            .optional()
            .unwrap();
        history.push(json!({"response":response, "manifest":state}));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "sensitive fixture preparation deadline: {history:?}"
        );
        if let Some(state) = state {
            if state.source_kind == "excluded"
                && state.sensitivity_class.as_deref() == Some("credential")
                && state.content_sha256.is_none()
            {
                client.close_cleanly();
                return;
            }
            assert!(
                state.source_kind == "pdf"
                    && state.index_status == "pending"
                    && state.content_sha256.as_deref() == Some(hash)
                    && state.sensitivity_class.is_none()
                    && state.last_error.is_none(),
                "sensitive fixture reached a terminal unexpected state: {history:?}"
            );
        }
    }
    panic!("sensitive fixture preparation request cap: {history:?}");
}

#[test]
fn cli_unicode_pdf_export_preserves_exact_bytes_and_current_authorization() {
    let _source_capacity = support::sources::source_test_guard();
    for transition in [
        "same_size_replacement",
        "magnesiumignore",
        "nested_workspace",
        "symlink",
    ] {
        if transition == "symlink" && !cfg!(unix) {
            continue;
        }
        let (env, _) = written_workspace();
        let pdf = valid_pdf("Alpha WAL end mark payload");
        let relative = format!("{}/{}.pdf", "é".repeat(70), "é".repeat(70));
        assert_eq!(relative.chars().count(), 145);
        assert_eq!(relative.len(), 285);
        let path = env.workspace.join(&relative);
        fs::create_dir(path.parent().unwrap()).unwrap();
        fs::write(&path, &pdf).unwrap();
        let (hash, preparation) = prepare_pdf_source(&env, &relative, &pdf);
        let inspected = v2::tool(
            &env,
            "codex",
            "export-thread",
            "sources",
            json!({"action":"inspect", "path":relative, "content_sha256":hash}),
        );
        assert_eq!(inspected["anchor"]["kind"], "page", "{inspected}");
        assert_eq!(inspected["coverage"]["status"], "complete", "{inspected}");
        assert_eq!(inspected["original"]["content_sha256"], hash, "{inspected}");
        assert_eq!(inspected["original"]["file_size"], pdf.len(), "{inspected}");
        assert!(
            inspected["excerpt"]
                .as_str()
                .unwrap()
                .contains("Alpha WAL end mark payload"),
            "{inspected}"
        );
        record_pdf_reference(&env, &relative, &hash);
        let ordinary = env.root.join("pdf-export-no-optin");
        successful(export(&env, &ordinary, &[]));
        assert!(!ordinary.join("sources").exists());
        let first = env.root.join("pdf-export-before");
        successful(export(&env, &first, &["--include-source-copies"]));
        let copied = first.join("sources").join(&relative);
        assert_eq!(fs::read(&copied).unwrap(), pdf, "{preparation:?}");
        assert_eq!(
            support::sources::fixture_hash(fs::read(&copied).unwrap()),
            hash
        );
        let before = read_json(&first.join("manifest.json"));
        assert!(
            before["source_coverage"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == relative
                    && entry["requested_hash"] == hash
                    && entry["status"] == "copied_current"),
            "{before}"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&copied).unwrap().permissions().mode() & 0o077,
                0
            );
        }
        match transition {
            "same_size_replacement" => {
                let mut changed = pdf.clone();
                let at = changed
                    .windows(5)
                    .position(|bytes| bytes == b"Alpha")
                    .unwrap();
                changed[at..at + 5].copy_from_slice(b"Beta!");
                assert_eq!(changed.len(), pdf.len());
                fs::write(&path, changed).unwrap();
            }
            "magnesiumignore" => {
                fs::write(
                    env.workspace
                        .join(support::CONTROL_DIR)
                        .join("magnesiumignore"),
                    format!("ignored.txt\n{relative}\n"),
                )
                .unwrap();
            }
            "nested_workspace" => {
                fs::create_dir(path.parent().unwrap().join(support::CONTROL_DIR)).unwrap();
            }
            "symlink" => {
                #[cfg(unix)]
                {
                    fs::remove_file(&path).unwrap();
                    let outside = env.root.join("same-hash.pdf");
                    fs::write(&outside, &pdf).unwrap();
                    std::os::unix::fs::symlink(outside, &path).unwrap();
                }
            }
            _ => unreachable!(),
        }
        let refused = v2::tool(
            &env,
            "codex",
            "export-thread",
            "sources",
            json!({"action":"inspect", "path":relative, "content_sha256":hash}),
        );
        assert_eq!(
            refused["coverage"]["status"],
            if transition == "same_size_replacement" {
                "stale"
            } else {
                "excluded"
            },
            "{transition}: {refused}"
        );
        assert!(
            refused.get("original").is_none() && refused.get("excerpt").is_none(),
            "{refused}"
        );
        let after = env.root.join("pdf-export-after");
        successful(export(&env, &after, &["--include-source-copies"]));
        assert!(
            !after.join("sources").join(&relative).exists(),
            "{transition}"
        );
        let manifest = read_json(&after.join("manifest.json"));
        let coverage = manifest["source_coverage"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == relative)
            .unwrap();
        assert_eq!(
            coverage["status"], "unavailable",
            "{transition}: {coverage}"
        );
        assert_eq!(coverage["requested_hash"], hash);
        assert_eq!(
            coverage["reason"],
            if transition == "same_size_replacement" {
                "source_changed"
            } else {
                "source_excluded"
            },
            "{transition}: {coverage}"
        );
        assert_eq!(fs::read(&copied).unwrap(), pdf);
        let overwritten = successful(export(
            &env,
            &first,
            &["--include-source-copies", "--overwrite"],
        ));
        let backup = Path::new(overwritten["previous_export_backup"].as_str().unwrap());
        assert_eq!(
            fs::read(backup.join("sources").join(&relative)).unwrap(),
            pdf
        );
        assert!(!first.join("sources").join(&relative).exists());
    }
}

#[test]
fn cli_sensitive_compressed_pdf_refuses_copy_despite_matching_raw_hash() {
    let _source_capacity = support::sources::source_test_guard();
    let (env, _) = written_workspace();
    let secret = "password: fixture-pdf-secret";
    let pdf = pdf_with_compression(&format!("{secret} ").repeat(24), true);
    assert!(
        !pdf.windows(secret.len())
            .any(|bytes| bytes == secret.as_bytes())
    );
    fs::write(env.workspace.join("compressed.pdf"), &pdf).unwrap();
    let hash = support::sources::fixture_hash(&pdf);
    prepare_sensitive_pdf_source(&env, "compressed.pdf", &hash);
    record_pdf_reference(&env, "compressed.pdf", &hash);
    let destination = env.root.join("sensitive-pdf-export");
    successful(export(&env, &destination, &["--include-source-copies"]));
    assert!(!destination.join("sources/compressed.pdf").exists());
    let manifest = read_json(&destination.join("manifest.json"));
    let coverage = manifest["source_coverage"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "compressed.pdf")
        .unwrap();
    assert_eq!(coverage["requested_hash"], hash);
    assert_eq!(coverage["status"], "unavailable", "{coverage}");
    assert_eq!(
        coverage["reason"], "pdf_source_not_authorized",
        "{coverage}"
    );
    let index = rusqlite::Connection::open(
        env.workspace
            .join(support::CONTROL_DIR)
            .join("index/source-search.sqlite"),
    )
    .unwrap();
    let (kind, sensitivity, stored_hash): (String, Option<String>, Option<String>) = index.query_row("SELECT source_kind, sensitivity_class, content_sha256 FROM source_manifest WHERE display_path = 'compressed.pdf'", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    assert_eq!(
        (kind.as_str(), sensitivity.as_deref(), stored_hash),
        ("excluded", Some("credential"), None)
    );
    assert_eq!(fs::read(env.workspace.join("compressed.pdf")).unwrap(), pdf);
}
