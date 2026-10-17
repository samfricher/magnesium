use std::{collections::BTreeSet, fs, path::Path};

use serde_json::Value;
use sha2::{Digest, Sha256};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/workspace/learning");
const PACKAGED_BUNDLE: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/learning-instructions-v2.bin"));
const FILES: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    ("MAGNESIUM.md", "orchestrator", None, None),
    ("teaching-policy.md", "policy", None, None),
    ("contracts/state-and-evidence.md", "contract", None, None),
    ("contracts/source-use.md", "contract", None, None),
    ("contracts/tool-recipes.md", "contract", None, None),
    (
        "workflows/orient-and-probe.md",
        "workflow",
        Some("orient-and-probe"),
        None,
    ),
    (
        "workflows/plan-and-route.md",
        "workflow",
        Some("plan-and-route"),
        None,
    ),
    (
        "workflows/teach-and-practice.md",
        "workflow",
        Some("teach-and-practice"),
        None,
    ),
    (
        "workflows/interpret-and-repair.md",
        "workflow",
        Some("interpret-and-repair"),
        None,
    ),
    (
        "workflows/review-and-transfer.md",
        "workflow",
        Some("review-and-transfer"),
        None,
    ),
    (
        "workflows/consolidate-and-resume.md",
        "workflow",
        Some("consolidate-and-resume"),
        None,
    ),
    ("workflows/papers.md", "workflow", Some("papers"), None),
    (
        "workflows/papers-loop.md",
        "workflow",
        Some("papers-loop"),
        None,
    ),
    ("adapters/codex.md", "adapter", None, Some("codex")),
    ("adapters/claude.md", "adapter", None, Some("claude")),
    ("adapters/opencode.md", "adapter", None, Some("opencode")),
];

fn sha256(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(hex, "{byte:02x}").expect("write to String");
    }
    hex
}

fn packaged_entries() -> Vec<(String, Vec<u8>)> {
    let mut cursor = PACKAGED_BUNDLE;
    let mut entries = Vec::new();
    while !cursor.is_empty() {
        let name_end = cursor
            .iter()
            .position(|byte| *byte == 0)
            .expect("package entry has a path terminator");
        let name = std::str::from_utf8(&cursor[..name_end])
            .expect("package path is UTF-8")
            .to_owned();
        cursor = &cursor[name_end + 1..];
        let length = u64::from_be_bytes(cursor[..8].try_into().expect("package entry length"));
        cursor = &cursor[8..];
        let length = usize::try_from(length).expect("package entry fits usize");
        let bytes = cursor[..length].to_vec();
        cursor = &cursor[length..];
        entries.push((name, bytes));
    }
    entries
}

#[test]
fn authored_files_match_active_manifest_and_packaged_bytes() {
    let root = Path::new(ROOT);
    let manifest_bytes = fs::read(root.join("manifest.json")).expect("manifest");
    let manifest: Value = serde_json::from_slice(&manifest_bytes).expect("manifest JSON");
    assert_eq!(manifest["bundle_version"], 2);
    assert_eq!(manifest["status"], "active");
    assert_eq!(manifest["minimum_learning_step_schema"], 2);
    assert_eq!(manifest["entrypoint"], "MAGNESIUM.md");

    let declared = manifest["files"].as_array().expect("explicit files array");
    assert_eq!(declared.len(), 16);
    let mut seen = BTreeSet::new();
    for (entry, &(relative, role, stage, host)) in declared.iter().zip(FILES) {
        assert_eq!(entry["path"], relative);
        assert_eq!(entry["role"], role);
        assert_eq!(entry.get("stage").and_then(Value::as_str), stage);
        assert_eq!(entry.get("host").and_then(Value::as_str), host);
        assert_eq!(entry["required_for_activation"], true);
        assert!(seen.insert(relative), "duplicate manifest path: {relative}");
        let path = root.join(relative);
        assert!(
            fs::symlink_metadata(&path)
                .expect("declared file exists")
                .file_type()
                .is_file(),
            "authored asset must be an ordinary file: {relative}"
        );
        let bytes = fs::read(&path).expect("authored file");
        assert!(!bytes.is_empty(), "authored asset is empty: {relative}");
        assert_eq!(entry["sha256"], sha256(&bytes));
    }
    assert_eq!(seen.len(), FILES.len());

    let mut actual = BTreeSet::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        for entry in fs::read_dir(directory).expect("bundle directory") {
            let entry = entry.expect("bundle entry");
            let kind = entry.file_type().expect("bundle entry type");
            assert!(!kind.is_symlink(), "bundle must not contain symlinks");
            if kind.is_dir() {
                directories.push(entry.path());
            } else {
                assert!(kind.is_file(), "bundle contains an unsupported entry");
                actual.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .expect("relative bundle path")
                        .to_str()
                        .expect("UTF-8 bundle path")
                        .replace('\\', "/"),
                );
            }
        }
    }
    let expected: BTreeSet<_> = FILES
        .iter()
        .map(|(path, ..)| (*path).to_owned())
        .chain(std::iter::once("manifest.json".to_owned()))
        .collect();
    assert_eq!(actual, expected);

    let packaged = packaged_entries();
    assert_eq!(packaged.len(), 17);
    assert_eq!(packaged[0], ("manifest.json".to_owned(), manifest_bytes));
    for ((name, bytes), (relative, ..)) in packaged.iter().skip(1).zip(FILES) {
        assert_eq!(name, relative);
        assert_eq!(bytes, &fs::read(root.join(relative)).unwrap());
    }
}

#[test]
fn installed_workspace_instructions_load_the_approved_policy() {
    let bridge = include_str!("../src/workspace/instructions.md");
    assert!(bridge.contains("teaching-policy/v2"));
    assert!(bridge.contains("context(action:\"instructions\""));
    assert!(!bridge.contains("teaching-policy/v1"));
}

#[test]
fn active_policy_uses_descriptive_memory_without_retired_authority_gates() {
    let root = Path::new(ROOT);
    let text = FILES
        .iter()
        .map(|(path, ..)| fs::read_to_string(root.join(path)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    for required in [
        "cross-conversation",
        "current_basis",
        "no_change",
        "review_resolution",
        "model_reported",
        "recording_off",
        "without mandatory retesting",
        "Stop retrieving once enough context",
        "same operation and context",
    ] {
        assert!(text.contains(required), "missing memory policy: {required}");
    }
    for retired in [
        "schema_version:3",
        "learning_step` v3",
        "attestation_id",
        "readiness_cursor",
        "prepare_forget",
        "confirm_forget_session",
        "not_enabled_policy_pending",
        "registered verifier",
        "same-conversation judgment",
    ] {
        assert!(!text.contains(retired), "retired instruction: {retired}");
    }
}

#[test]
fn info_reports_actual_installed_bundle_integrity() {
    let environment = support::TestEnvironment::from_fixture("basic").unwrap();
    assert!(
        environment
            .command()
            .arg("init")
            .output()
            .unwrap()
            .status
            .success()
    );
    let read = || {
        let output = environment
            .command()
            .args(["info", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let status = read();
    assert_eq!(status["instructions"]["status"], "current");
    let hash = status["instructions"]["bundle_hash"].as_str().unwrap();
    assert_eq!(hash.len(), 64);
    let entry = environment
        .workspace
        .join(support::CONTROL_DIR)
        .join("instructions/v2")
        .join(hash)
        .join("MAGNESIUM.md");
    fs::write(&entry, "locally changed policy").unwrap();
    assert_eq!(read()["instructions"]["status"], "unavailable");
    assert_eq!(fs::read_to_string(entry).unwrap(), "locally changed policy");
}
