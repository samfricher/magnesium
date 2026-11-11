//! Course material added from the window, against the runtime built from this
//! same checkout: a PDF dropped on a course is in the study folder and the
//! tutor's own search finds a phrase from it; a scan, a file left out by a
//! word in its name, and a kind search does not read are each said in a line;
//! an answer key is added and never named. A folder that was never set up is
//! judged by the rules it will have, and nothing sets it up.

#![cfg(unix)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use mg_core::claude::ClaudeCode;
use mg_core::{Manager, Store};
use serde_json::{json, Value};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the repository root")
}

/// The runtime of this checkout, as `cargo build` at the root makes it.
/// `MAGNESIUM_TEST_BINARY` names one already built.
fn runtime_binary() -> PathBuf {
    if let Some(binary) = std::env::var_os("MAGNESIUM_TEST_BINARY").filter(|v| !v.is_empty()) {
        return PathBuf::from(binary);
    }
    let root = repo_root();
    let status = Command::new("cargo")
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .args(["build", "--release", "--locked", "--bin", "magnesium"])
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the runtime did not build");
    root.join("target/release/magnesium")
}

/// A one-page PDF with a text layer, or with a blank page (a scan, as a
/// text reader sees it).
fn pdf(text: Option<&str>) -> Vec<u8> {
    let content = text
        .map(|t| format!("BT /F1 18 Tf 72 700 Td ({t}) Tj ET"))
        .unwrap_or_default();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len() + 1),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        write!(bytes, "{} 0 obj\n{object}\nendobj\n", index + 1).unwrap();
    }
    let xref = bytes.len();
    write!(
        bytes,
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    )
    .unwrap();
    for offset in offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    )
    .unwrap();
    bytes
}

/// The tests choose the runtime through one environment variable, so they
/// take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|p| p.into_inner())
}

struct Scratch {
    _dir: tempfile::TempDir,
    root: PathBuf,
    folder: PathBuf,
    binary: PathBuf,
}

impl Scratch {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for name in [
            "home", "data", "config", "tmp", "window", "Course", "dropped",
        ] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        let folder = root.join("Course");
        let binary = runtime_binary();
        let scratch = Self {
            _dir: dir,
            root,
            folder,
            binary,
        };
        std::env::set_var(mg_core::runtime::PROGRAM_ENV, scratch.wrapper());
        scratch
    }

    fn environment(&self) -> Vec<(String, String)> {
        let path = |name: &str| self.root.join(name).to_string_lossy().into_owned();
        vec![
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("HOME".to_owned(), path("home")),
            ("XDG_DATA_HOME".to_owned(), path("data")),
            ("XDG_CONFIG_HOME".to_owned(), path("config")),
            ("TMPDIR".to_owned(), path("tmp")),
            ("CLAUDE_CODE_SESSION_ID".to_owned(), "material".to_owned()),
        ]
    }

    /// A program the window can run as `magnesium`: the runtime, in a
    /// scratch home.
    fn wrapper(&self) -> PathBuf {
        let wrapper = self.root.join("magnesium");
        let mut script = String::from("#!/bin/sh\nexec env -i");
        for (name, value) in self.environment() {
            script.push_str(&format!(" {name}='{value}'"));
        }
        script.push_str(&format!(" '{}' \"$@\"\n", self.binary.display()));
        fs::write(&wrapper, script).unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
        wrapper
    }

    fn drop_file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join("dropped").join(name);
        fs::write(&path, bytes).unwrap();
        path
    }

    fn manager(&self) -> Manager {
        let manager = Manager::new(
            Arc::new(Store::at(self.root.join("window"))),
            Arc::new(ClaudeCode::with_program(self.root.join("no-agent"))),
            Arc::new(|_| {}),
        );
        manager.open_folder(&self.folder).unwrap();
        manager
    }

    /// What the tutor's own search returns for a phrase, once its index has caught up.
    fn search(&self, query: &str) -> Vec<Value> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let mut child = Command::new(&self.binary)
                .env_clear()
                .envs(self.environment())
                .current_dir(&self.folder)
                .args([
                    "adapter",
                    "call",
                    "search_sources",
                    "--protocol-version",
                    "2",
                    "--json-input",
                    "-",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(json!({"query": query, "limit": 5}).to_string().as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            let packet: Value = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
            let hits = packet["hits"].as_array().cloned().unwrap_or_default();
            if !hits.is_empty() || Instant::now() > deadline {
                return hits;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
}

#[test]
fn a_pdf_dropped_on_a_course_is_found_by_the_tutors_search_and_a_scan_is_said() {
    let _alone = alone();
    let scratch = Scratch::new();
    Command::new(scratch.root.join("magnesium"))
        .current_dir(&scratch.folder)
        .args(["init", "--json"])
        .output()
        .unwrap();
    let manager = scratch.manager();

    let added = manager
        .add_material(&[
            scratch.drop_file(
                "Week 4 slides.pdf",
                &pdf(Some(
                    "The posterior is proportional to the prior times the likelihood",
                )),
            ),
            scratch.drop_file("Scanned notes.pdf", &pdf(None)),
            scratch.drop_file("api tokens.md", b"posterior"),
            scratch.drop_file("Week 4 deck.pptx", b"x"),
            scratch.drop_file("Midterm answer key.pdf", &pdf(Some("secret marking guide"))),
        ])
        .unwrap();

    // Each file is in the study folder, under the same name.
    for name in [
        "Week 4 slides.pdf",
        "Scanned notes.pdf",
        "api tokens.md",
        "Week 4 deck.pptx",
        "Midterm answer key.pdf",
    ] {
        assert!(
            scratch.folder.join("Study/materials").join(name).is_file(),
            "{name}"
        );
    }
    // The key is added and never named.
    assert_eq!(added.keys, 1);
    assert_eq!(added.saved.len(), 4, "{added:?}");
    let said = added.lines.join("\n");
    assert!(!said.contains("answer key.pdf"), "{said}");
    // The three things search cannot read are each said, in plain words.
    assert!(
        said.contains(
            "\"Scanned notes.pdf\" is a scan with no text, so its text cannot be searched."
        ),
        "{said}"
    );
    assert!(
        said.contains("\"api tokens.md\" because its name has the word \"tokens\""),
        "{said}"
    );
    assert!(said.contains("Search does not read .pptx files"), "{said}");
    assert!(!said.contains("Week 4 slides.pdf\" is a scan"), "{said}");

    // The tutor's own search finds a phrase from the PDF, at its page.
    let hits = scratch.search("posterior proportional prior likelihood");
    let hit = hits
        .iter()
        .find(|hit| hit["path"] == "Study/materials/Week 4 slides.pdf")
        .unwrap_or_else(|| panic!("search did not find the dropped PDF: {hits:?}"));
    assert_eq!(hit["anchor_kind"], "page");
    assert_eq!(hit["anchor_start"], 1);
    // And it does not find what was said to be left out.
    assert!(scratch
        .search("posterior")
        .iter()
        .all(|hit| hit["path"] != "Study/materials/api tokens.md"));
}

#[test]
fn a_folder_that_was_never_set_up_is_judged_and_left_as_it_was() {
    let _alone = alone();
    let scratch = Scratch::new();
    let manager = scratch.manager();
    let added = manager
        .add_material(&[scratch.drop_file("scan.pdf", &pdf(None))])
        .unwrap();
    assert_eq!(
        added.lines,
        [
            "Added \"scan.pdf\" to Study/materials.",
            "\"scan.pdf\" is a scan with no text, so its text cannot be searched."
        ]
    );
    assert!(scratch.folder.join("Study/materials/scan.pdf").is_file());
    assert!(
        !scratch.folder.join(".magnesium").exists(),
        "adding material does not set the folder up"
    );

    let pasted = manager
        .add_pasted_text("Bayes' rule links the prior and the posterior.")
        .unwrap();
    assert!(
        pasted.lines[0].starts_with("Saved your pasted text as \"Pasted text "),
        "{pasted:?}"
    );
    assert_eq!(pasted.lines[1], "Search can read it.");
}

#[test]
fn an_older_runtime_with_no_such_command_is_admitted_to() {
    let _alone = alone();
    let dir = tempfile::tempdir().unwrap();
    let program = dir.path().join("magnesium");
    fs::write(
        &program,
        "#!/bin/sh\ncase \"$1\" in digest) echo '{\"active\":false}';; *) echo 'unknown command' >&2; exit 2;; esac\n",
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    std::env::set_var(mg_core::runtime::PROGRAM_ENV, &program);
    let folder = dir.path().join("Course");
    fs::create_dir_all(&folder).unwrap();
    let source = dir.path().join("notes.md");
    fs::write(&source, "x").unwrap();
    let manager = Manager::new(
        Arc::new(Store::at(dir.path().join("window"))),
        Arc::new(ClaudeCode::with_program(dir.path().join("no-agent"))),
        Arc::new(|_| {}),
    );
    manager.open_folder(&folder).unwrap();
    let added = manager.add_material(&[source]).unwrap();
    assert!(
        added
            .lines
            .iter()
            .any(|line| line.contains("cannot say whether search can read")),
        "{added:?}"
    );
    assert!(folder.join("Study/materials/notes.md").is_file());
}
