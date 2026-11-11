use super::*;
use crate::learner::{HostContext, RawEvent};
struct Fixture {
    root: PathBuf,
    workspace: Workspace,
    store: LearnerStore,
    host: HostContext,
    memory: String,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(crate::learner::store::new_id("consumer-test").unwrap());
        fs::create_dir_all(root.join("workspace").join(crate::workspace::CONTROL_DIR)).unwrap();
        fs::write(
            root.join("workspace")
                .join(crate::workspace::CONTROL_DIR)
                .join("config.toml"),
            "schema_version=1\nworkspace_id='consumer-test'\n",
        )
        .unwrap();
        let workspace = workspace(&root.join("workspace")).unwrap();
        let store = LearnerStore::initialize(&workspace.store_path(), "consumer-test").unwrap();
        let host = HostContext {
            host_kind: "test".into(),
            host_thread_id: "monday".into(),
        };
        store.update(&host,json!({"schema":"learner-model/v2","action":"update","request_id":"start","session":{"operation":"start","client_ref":"s","goal":"quadratics"}})).unwrap();
        let event = store
            .capture(
                &host,
                &[RawEvent {
                    external_event_key: "q1".into(),
                    speaker: crate::learner::Speaker::Learner,
                    kind: "message".into(),
                    text: "I applied the formula correctly, but what does b stand for?".into(),
                    metadata: Some(crate::learner::EventMetadata::default()),
                    occurred_at_ms: Some(100),
                }],
                "model_reported",
                false,
            )
            .unwrap()
            .remove(0);
        let result=store.update(&host,json!({"schema":"learner-model/v2","action":"update","request_id":"memory","turn_ref":event,"observations":[{"client_ref":"o","kind":"learner_question","description":"Formula substitution is available; coefficient roles uncertain.","events":[{"event_id":event,"role":"learner_response","excerpts":[]}]}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":"Coefficient roles","scope":"Quadratics","summary":"Formula application works; symbol roles need explanation.","state_text":"Preserve the substitution strength. The question does not establish a broad algebra difficulty.","current_basis":[{"new_observation":"o"}]},"reason":"Learner question","change_basis":[{"new_observation":"o"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})).unwrap();
        Self {
            root,
            workspace,
            store,
            host,
            memory: result.references["m"].clone(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn export_uses_writer_records_and_exact_prepared_context() {
    let f = Fixture::new();
    let packet = crate::learner::read::test_support::execute(
        &f.store.conn,
        "test",
        "monday",
        json!({"schema":"learner-model/v2","action":"prepare"}),
    )
    .unwrap();
    let later = f
        .store
        .capture(
            &f.host,
            &[RawEvent {
                external_event_key: "later-challenge".into(),
                speaker: crate::learner::Speaker::Learner,
                kind: "message".into(),
                text: "Please revisit whether I truly understand b.".into(),
                metadata: Some(crate::learner::EventMetadata::default()),
                occurred_at_ms: Some(200),
            }],
            "model_reported",
            false,
        )
        .unwrap()
        .remove(0);
    f.store.update(&f.host,json!({"schema":"learner-model/v2","action":"update","request_id":"later-review","turn_ref":later,"memory_edits":[{"operation":"flag","memory_id":f.memory,"expected_revision":1,"review_note":"Recheck coefficient understanding","reason":"Learner requests reconsideration","change_basis":[{"event_id":later}]}],"event_dispositions":[{"event_id":later,"outcome":"processed"}]})).unwrap();
    let destination = f.root.join("export");
    export_learner_data(
        f.workspace.root(),
        &ExportOptions {
            destination: destination.clone(),
            format: ExportFormat::Dir,
            include_source_copies: false,
            overwrite: false,
        },
    )
    .unwrap();
    let data: Value =
        serde_json::from_slice(&fs::read(destination.join("learner.json")).unwrap()).unwrap();
    assert_eq!(data["learner_memories"][0]["memory_id"], f.memory);
    assert_eq!(data["learner_memories"][0]["revision"], 2);
    assert_eq!(data["memory_changes"].as_array().unwrap().len(), 2);
    assert_eq!(data["learning_events"][0]["origin"], "model_reported");
    let saved: Value = serde_json::from_str(
        data["context_receipts"][0]["response_json"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved, packet);
    assert_eq!(data["memory_changes"][0]["revision"], 1);
    assert!(!data.as_object().unwrap().contains_key("capability_states"));
    assert!(
        fs::read_to_string(destination.join("README.md"))
            .unwrap()
            .contains("substitution strength")
    );
}
#[test]
fn zip_and_existing_destination_are_safe() {
    let f = Fixture::new();
    let destination = f.root.join("export.zip");
    let options = ExportOptions {
        destination: destination.clone(),
        format: ExportFormat::Zip,
        include_source_copies: false,
        overwrite: false,
    };
    export_learner_data(f.workspace.root(), &options).unwrap();
    let bytes = fs::read(&destination).unwrap();
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert!(export_learner_data(f.workspace.root(), &options).is_err());
    assert_eq!(fs::read(&destination).unwrap(), bytes);
}
impl Fixture {
    fn add_memory(&self, title: &str, sources: Value) -> String {
        let key = crate::learner::store::new_id("evidence").unwrap();
        let event = self
            .store
            .capture(
                &self.host,
                &[RawEvent {
                    external_event_key: key.clone(),
                    speaker: crate::learner::Speaker::Learner,
                    kind: "message".into(),
                    text: format!("My explanation about {title}"),
                    metadata: Some(crate::learner::EventMetadata::default()),
                    occurred_at_ms: None,
                }],
                "model_reported",
                false,
            )
            .unwrap()
            .remove(0);
        let result=self.store.update(&self.host,json!({"schema":"learner-model/v2","action":"update","request_id":key,"turn_ref":event,"observations":[{"client_ref":"o","kind":"learner_question","description":title,"events":[{"event_id":event,"role":"learner_response","excerpts":[]}],"source_refs":sources}],"memory_edits":[{"operation":"create","client_ref":"m","record":{"title":title,"scope":"algebra","summary":title,"state_text":title,"current_basis":[{"new_observation":"o"}]},"reason":"New learner explanation","change_basis":[{"new_observation":"o"}]}],"event_dispositions":[{"event_id":event,"outcome":"processed"}]})).unwrap();
        result.references["m"].clone()
    }
    fn export(&self, name: &str, copies: bool, overwrite: bool) -> Result<Value, String> {
        export_learner_data(
            self.workspace.root(),
            &ExportOptions {
                destination: self.root.join(name),
                format: ExportFormat::Dir,
                include_source_copies: copies,
                overwrite,
            },
        )
    }
}

#[test]
fn cancelled_copy_read_staging_and_publication_preserve_previous_export() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    for checkpoint in [
        "export_before_source_copy",
        "sources_current_read_chunk",
        "export_before_stage_write",
        "export_before_publication",
    ] {
        let f = Fixture::new();
        fs::write(
            f.workspace.root().join("allowed.txt"),
            "coefficient explanation",
        )
        .unwrap();
        f.add_memory(
            "Source exercise",
            json!([{"path":"allowed.txt","available_form":"original"}]),
        );
        f.export("replace", false, false).unwrap();
        fs::write(f.root.join("replace/user-note.txt"), "keep user addition").unwrap();
        let before = fs::read(f.root.join("replace/learner.json")).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let flag_hook = flag.clone();
        let token = crate::retrieval::OperationCancellation::with_flag(flag.clone())
            .with_test_hook(Arc::new(move |observed| {
                if observed == checkpoint {
                    flag_hook.store(true, Ordering::Relaxed);
                }
            }));
        let result = {
            let _scope = crate::request::enter_request_cancellation(&token);
            f.export("replace", true, true)
        };
        assert!(
            flag.load(Ordering::Relaxed),
            "checkpoint was reached: {checkpoint}"
        );
        assert!(
            result.unwrap_err().contains("source_operation_cancelled"),
            "{checkpoint}"
        );
        assert_eq!(
            fs::read(f.root.join("replace/learner.json")).unwrap(),
            before
        );
        assert_eq!(
            fs::read_to_string(f.root.join("replace/user-note.txt")).unwrap(),
            "keep user addition"
        );
        assert!(!f.root.join("replace/sources/allowed.txt").exists());
        assert!(!fs::read_dir(&f.root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".magnesium-export-stage")
        }));
    }
}

#[test]
fn optional_pdf_copy_propagates_cache_failure_and_halts_later_optional_work() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let f = Fixture::new();
    let pdf = include_bytes!("../../tests/fixtures/interaction/statistics/source-pages.pdf");
    fs::write(f.workspace.root().join("source.pdf"), pdf).unwrap();
    fs::write(
        f.workspace.root().join("later.txt"),
        "later optional source",
    )
    .unwrap();
    f.add_memory(
        "Source exercise",
        json!([
            {"path":"missing.txt","available_form":"original"},
            {"path":"source.pdf","available_form":"original"},
            {"path":"later.txt","available_form":"original"}
        ]),
    );
    let index = f
        .workspace
        .root()
        .join(crate::workspace::CONTROL_DIR)
        .join("index/source-search.sqlite");
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    Connection::open(&index)
        .unwrap()
        .execute_batch(
            "CREATE TABLE foreign_data(value TEXT); INSERT INTO foreign_data VALUES ('preserve');",
        )
        .unwrap();
    let cache = fs::read(&index).unwrap();
    f.export("replace", false, false).unwrap();
    let previous = fs::read(f.root.join("replace/learner.json")).unwrap();
    let observed = Arc::new(AtomicUsize::new(0));
    let observed_hook = observed.clone();
    let token = crate::retrieval::OperationCancellation::none().with_test_hook(Arc::new(
        move |checkpoint| {
            if checkpoint == "export_before_source_copy" {
                observed_hook.fetch_add(1, Ordering::Relaxed);
            }
        },
    ));
    let result = {
        let _scope = crate::request::enter_request_cancellation(&token);
        f.export("replace", true, true)
    };
    assert!(result.unwrap_err().contains("source-search.sqlite"));
    assert_eq!(observed.load(Ordering::Relaxed), 2);
    assert_eq!(fs::read(&index).unwrap(), cache);
    assert_eq!(
        fs::read(f.root.join("replace/learner.json")).unwrap(),
        previous
    );
    assert!(!f.root.join("replace/sources/later.txt").exists());
}

#[test]
fn actual_writer_source_refs_obey_exclusions_versions_and_copy_opt_in() {
    let f = Fixture::new();
    let root = f.workspace.root();
    fs::write(root.join("allowed.txt"), "coefficient explanation").unwrap();
    fs::write(root.join("ignored.txt"), "private lesson").unwrap();
    fs::write(
        root.join(crate::workspace::CONTROL_DIR)
            .join("magnesiumignore"),
        "ignored.txt\n",
    )
    .unwrap();
    fs::write(root.join(".env"), "TOKEN=private-value").unwrap();
    fs::write(
        root.join("secret.txt"),
        "-----BEGIN PRIVATE KEY-----\nprivate\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("nested").join(crate::workspace::CONTROL_DIR)).unwrap();
    fs::write(
        root.join("nested")
            .join(crate::workspace::CONTROL_DIR)
            .join("config.toml"),
        "schema_version=1\nworkspace_id='nested'",
    )
    .unwrap();
    fs::write(
        root.join("nested/private.txt"),
        "nested workspace private data",
    )
    .unwrap();
    let mut refs = vec![];
    for path in [
        "allowed.txt",
        "ignored.txt",
        ".env",
        "secret.txt",
        "nested/private.txt",
        "missing.txt",
    ] {
        refs.push(json!({"path":path,"available_form":"original"}));
    }
    refs.push(json!({"path":"allowed.txt","hash":"0".repeat(64),"available_form":"original"}));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("allowed.txt"), root.join("alias.txt")).unwrap();
        refs.push(json!({"path":"alias.txt","available_form":"original"}));
    }
    f.add_memory("Source exercise", json!(refs));
    f.export("no-copies", false, false).unwrap();
    assert!(!f.root.join("no-copies/sources").exists());
    f.export("with-copies", true, false).unwrap();
    assert_eq!(
        fs::read_to_string(f.root.join("with-copies/sources/allowed.txt")).unwrap(),
        "coefficient explanation"
    );
    for path in [
        "ignored.txt",
        ".env",
        "secret.txt",
        "nested/private.txt",
        "missing.txt",
        "alias.txt",
    ] {
        assert!(
            !f.root.join("with-copies/sources").join(path).exists(),
            "copied excluded {path}"
        );
    }
    let manifest: Value =
        serde_json::from_slice(&fs::read(f.root.join("with-copies/manifest.json")).unwrap())
            .unwrap();
    assert!(
        manifest["source_coverage"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["path"] == "allowed.txt" && entry["reason"] == "source_changed")
    );
}
#[test]
fn overwrite_preserves_old_artifact_and_user_additions_and_rejects_workspace_roots() {
    let f = Fixture::new();
    f.export("replace", false, false).unwrap();
    fs::write(f.root.join("replace/my-note.txt"), "keep my addition").unwrap();
    let updated = f.export("replace", false, true).unwrap();
    let backup = PathBuf::from(updated["previous_export_backup"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(backup.join("my-note.txt")).unwrap(),
        "keep my addition"
    );
    assert!(!f.root.join("replace/my-note.txt").exists());
    assert!(
        f.export("workspace", false, true)
            .unwrap_err()
            .contains("workspace root")
    );
    fs::create_dir(f.root.join("unrelated")).unwrap();
    assert!(
        f.export("unrelated", false, true)
            .unwrap_err()
            .contains("Magnesium export")
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(f.root.join("replace"), f.root.join("alias")).unwrap();
        assert!(
            f.export("alias", false, true)
                .unwrap_err()
                .contains("symlink")
        );
    }
}
