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
    ("adapters/claude.md", "adapter", None, Some("claude")),
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
fn packaged_bundle_holds_every_authored_file_and_its_hash() {
    let root = Path::new(ROOT);
    let authored: Value =
        serde_json::from_slice(&fs::read(root.join("manifest.json")).expect("manifest"))
            .expect("manifest JSON");
    assert_eq!(authored["bundle_version"], 2);
    assert_eq!(authored["status"], "active");
    assert_eq!(authored["minimum_learning_step_schema"], 2);
    assert_eq!(authored["entrypoint"], "MAGNESIUM.md");

    // The folder holds the manifest and the files it lists, and nothing else.
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

    // The pack is the manifest, with the hash the build worked out for each
    // file, then the files as authored and in the manifest's order.
    let packaged = packaged_entries();
    assert_eq!(packaged.len(), FILES.len() + 1);
    assert_eq!(packaged[0].0, "manifest.json");
    let mut manifest: Value = serde_json::from_slice(&packaged[0].1).expect("packaged manifest");
    let declared = manifest["files"].as_array_mut().expect("files array");
    assert_eq!(declared.len(), FILES.len());
    for ((entry, &(relative, role, stage, host)), (name, bytes)) in
        declared.iter_mut().zip(FILES).zip(packaged.iter().skip(1))
    {
        assert_eq!(entry["path"], relative);
        assert_eq!(entry["role"], role);
        assert_eq!(entry.get("stage").and_then(Value::as_str), stage);
        assert_eq!(entry.get("host").and_then(Value::as_str), host);
        assert_eq!(entry["required_for_activation"], true);
        assert_eq!(name, relative);
        let authored_bytes = fs::read(root.join(relative)).expect("authored file");
        assert!(
            !authored_bytes.is_empty(),
            "authored file is empty: {relative}"
        );
        assert_eq!(bytes, &authored_bytes);
        let hash = entry.as_object_mut().unwrap().remove("sha256");
        assert_eq!(hash, Some(Value::from(sha256(&authored_bytes))));
    }
    assert_eq!(manifest, authored, "the build adds only the hashes");
}

#[test]
fn installed_workspace_instructions_load_the_approved_policy() {
    let bridge = include_str!("../../src/workspace/instructions.md");
    assert!(bridge.contains("teaching-policy/v2"));
    assert!(bridge.contains("context(action:\"instructions\""));
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
    assert!(
        !text.contains("learning_step` v3"),
        "retired instruction: learning_step` v3"
    );
}

#[test]
fn info_reports_actual_installed_bundle_integrity() {
    let environment = crate::support::TestEnvironment::from_fixture("basic").unwrap();
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
        .join(crate::support::CONTROL_DIR)
        .join("instructions/v2")
        .join(hash)
        .join("MAGNESIUM.md");
    fs::write(&entry, "locally changed policy").unwrap();
    assert_eq!(read()["instructions"]["status"], "unavailable");
    assert_eq!(fs::read_to_string(entry).unwrap(), "locally changed policy");
}
