use std::env;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

use pdf_extract::{Document, Object, Stream, content::Content, content::Operation, dictionary};
use rusqlite::Connection;
use serde_json::Value;
use support::test_binary;
use tree_sitter::Parser;

#[test]
fn fresh_runtime_store_matches_the_runtime_and_holds_no_learner_rows() {
    let environment =
        support::TestEnvironment::from_fixture("basic").expect("create isolated runtime workspace");
    let home = environment.root.join("home");
    let report = initialize_and_inspect(
        &test_binary(),
        &environment.workspace,
        &home,
        &environment.global_data,
        &environment.root,
    );
    assert_fresh_store_contract(
        &report,
        &environment.workspace,
        &home,
        &environment.global_data,
    );
}

fn initialize_and_inspect(
    executable: &Path,
    workspace: &Path,
    home: &Path,
    global: &Path,
    prefix: &Path,
) -> Value {
    let mut init = Command::new(executable);
    init.current_dir(prefix)
        .args(["init", workspace.to_str().unwrap()]);
    isolated_env(&mut init, home, global, prefix);
    let output = init.output().expect("run isolated magnesium init");
    assert!(
        output.status.success(),
        "isolated init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut info = Command::new(executable);
    info.current_dir(workspace).args(["info", "--json"]);
    isolated_env(&mut info, home, global, prefix);
    let output = info.output().expect("run isolated magnesium info");
    assert!(
        output.status.success(),
        "isolated info failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("isolated info JSON")
}

fn assert_fresh_store_contract(report: &Value, workspace: &Path, home: &Path, global: &Path) {
    assert_eq!(report["active"], true);
    assert_eq!(report["schema"], "learner-model/v2");
    assert_eq!(report["runtime_version"], env!("CARGO_PKG_VERSION"));
    let runtime_schema = report["schema_version"]
        .as_i64()
        .expect("runtime canonical schema version");
    assert_eq!(
        runtime_schema, 2,
        "learner-model/v2 stores use schema version 2"
    );
    assert!(
        workspace
            .join(support::CONTROL_DIR)
            .join("state.sqlite")
            .is_file(),
        "canonical state exists"
    );
    let state = Connection::open(workspace.join(support::CONTROL_DIR).join("state.sqlite"))
        .expect("open canonical state");
    let (workspace_id, storage_family, stored_schema): (String, String, i64) = state
        .query_row(
            "SELECT workspace_id, storage_family, schema_version FROM workspace_meta",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("canonical workspace metadata");
    assert_eq!(report["workspace_id"], workspace_id);
    assert_eq!(storage_family, "learner-model/v2");
    assert_eq!(stored_schema, runtime_schema);
    for table in [
        "learning_sessions",
        "host_bindings",
        "learning_events",
        "observations",
        "observation_events",
        "learner_memories",
        "memory_topics",
        "memory_observations",
        "memory_changes",
        "operation_receipts",
        "review_items",
        "context_receipts",
        "memory_fts",
        "observation_fts",
    ] {
        let count: i64 = state
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .expect("count canonical rows");
        assert_eq!(count, 0, "init must not create learner rows in {table}");
    }
    // Learner state stays in the workspace; the per-user data directory holds
    // only the plugin copies that init manages.
    for data_dir in [
        home.join("Library/Application Support/magnesium"),
        global.join("magnesium"),
    ] {
        let Ok(entries) = fs::read_dir(&data_dir) else {
            continue;
        };
        for entry in entries {
            let name = entry.unwrap().file_name();
            assert!(
                name == "marketplace" || name == "claude-plugin",
                "init wrote unmanaged global data: {}",
                data_dir.join(name).display()
            );
        }
    }
}

fn isolated_env(command: &mut Command, home: &Path, global: &Path, prefix: &Path) {
    command
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_DATA_HOME", global)
        .env("XDG_CONFIG_HOME", prefix.join("config"))
        .env("LOCALAPPDATA", global)
        .env("APPDATA", global)
        .env("CODEX_HOME", prefix.join("codex-home"))
        .env("CODEX_TEST_BINARY", prefix.join("codex-not-installed"))
        .env(
            "OPENCODE_TEST_BINARY",
            prefix.join("opencode-not-installed"),
        )
        .env("PATH", prefix.join("empty-path"))
        .env("TMPDIR", prefix.join("tmp"))
        .env("TEMP", prefix.join("tmp"))
        .env("TMP", prefix.join("tmp"));
    fs::create_dir_all(prefix.join("empty-path")).expect("create controlled PATH");
    fs::create_dir_all(prefix.join("tmp")).expect("create controlled temp");
}

#[test]
fn selected_native_content_dependencies_work_without_sidecars() {
    assert_parses(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "function twice(value: number): number { return value * 2; }",
        "function_declaration",
    );
    assert_parses(
        tree_sitter_javascript::LANGUAGE.into(),
        "export const answer = () => 42;",
        "export_statement",
    );
    assert_parses(
        tree_sitter_python::LANGUAGE.into(),
        "def twice(value):\n    return value * 2\n",
        "function_definition",
    );

    let pdf = readable_pdf_fixture();
    let started = Instant::now();
    let mut pages = Vec::new();
    for _ in 0..100 {
        pages = pdf_extract::extract_text_from_mem_by_pages(&pdf)
            .expect("extract readable PDF text by page");
    }
    eprintln!(
        "pdf-extract 0.12.0: 100 iterations, {} input bytes, {:?}",
        pdf.len(),
        started.elapsed()
    );
    assert_eq!(pages.len(), 2);
    assert!(pages[0].contains("Alpha page anchor"));
    assert!(pages[1].contains("Beta page anchor"));

    let runtime_pages = runtime_pdf_pages(&pdf);
    assert!(runtime_pages[0].contains("Alpha page anchor"));
    assert!(runtime_pages[1].contains("Beta page anchor"));
}

fn runtime_pdf_pages(pdf: &[u8]) -> Vec<String> {
    let mut child = Command::new(test_binary())
        .args(["_internal", "extract-pdf"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start release PDF extractor mode");
    child
        .stdin
        .take()
        .expect("piped PDF input")
        .write_all(pdf)
        .expect("write PDF input");
    let output = child.wait_with_output().expect("wait for PDF extractor");
    assert!(
        output.status.success(),
        "PDF extractor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker = b"MAGNESIUM_PDF_V2 ";
    assert!(output.stdout.starts_with(marker));
    let framed = &output.stdout[marker.len()..];
    let newline = framed
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("PDF frame length terminator");
    let length: usize = std::str::from_utf8(&framed[..newline])
        .expect("PDF frame length UTF-8")
        .parse()
        .expect("PDF frame length integer");
    let payload = &framed[newline + 1..];
    assert_eq!(payload.len(), length, "complete PDF frame");
    let reply: Value = serde_json::from_slice(payload).expect("PDF frame JSON");
    assert_eq!(reply["outcome"], "pages");
    assert_eq!(reply.as_object().expect("PDF reply").len(), 2);
    reply["pages"]
        .as_array()
        .expect("PDF pages")
        .iter()
        .map(|page| page.as_str().expect("PDF page text").to_owned())
        .collect()
}

fn assert_parses(language: tree_sitter::Language, source: &str, expected_kind: &str) {
    let mut parser = Parser::new();
    parser.set_language(&language).expect("load grammar");
    let tree = parser.parse(source, None).expect("parse source");
    assert!(!tree.root_node().has_error());
    assert!(
        tree.root_node()
            .named_children(&mut tree.walk())
            .any(|node| node.kind() == expected_kind),
        "parse tree omitted {expected_kind}: {}",
        tree.root_node().to_sexp()
    );
}

fn readable_pdf_fixture() -> Vec<u8> {
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

    let page_ids = ["Alpha page anchor", "Beta page anchor"].map(|text| {
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
        document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        })
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.map(Object::Reference).to_vec(),
            "Count" => 2,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    document.save_to(&mut bytes).expect("serialize PDF fixture");
    bytes
}
