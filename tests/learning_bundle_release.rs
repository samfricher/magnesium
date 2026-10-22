use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const CHECKER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/ci/check_learning_bundle_release.py"
);
const RESERVED: &[u8] = b"<!-- RESERVED_FOR_EXPERT_AUTHORING: DO_NOT_ACTIVATE -->\n";
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new("python3")
        .arg(CHECKER)
        .args(args)
        .output()
        .expect("run learning bundle release checker")
}

fn temp_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("magnesium-release-{name}-{}", std::process::id()))
}

fn production_entries() -> Vec<Value> {
    let manifest: Value = serde_json::from_slice(
        &fs::read(Path::new(ROOT).join("src/workspace/learning/manifest.json")).unwrap(),
    )
    .unwrap();
    manifest["files"].as_array().unwrap().clone()
}

fn write_candidate(root: &Path) -> Vec<u8> {
    let files = production_entries()
        .into_iter()
        .map(|metadata| {
            let path = metadata["path"].as_str().unwrap();
            let bytes = format!("synthetic gate fixture for {path}\n").into_bytes();
            let full_path = root.join(path);
            fs::create_dir_all(full_path.parent().unwrap()).unwrap();
            fs::write(full_path, &bytes).unwrap();
            let mut entry = metadata;
            entry["sha256"] = json!(hash(&bytes));
            entry
        })
        .collect::<Vec<_>>();
    let manifest = json!({
        "bundle_version": 2,
        "status": "active",
        "minimum_learning_step_schema": 2,
        "entrypoint": "MAGNESIUM.md",
        "files": files,
    });
    let bytes = serde_json::to_vec(&manifest).unwrap();
    fs::write(root.join("manifest.json"), &bytes).unwrap();
    bytes
}

#[test]
fn production_bundle_integrity_passes_without_author_approval() {
    let output = run(&["--source-root", ROOT]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn status_flip_marker_partial_and_changed_assets_fail() {
    let root = temp_path("candidate");
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let manifest = write_candidate(&root);
    let output = run(&["--candidate-bundle", root.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "synthetic complete fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for (original, replacement, metadata_field, metadata_value) in [
        (
            "adapters/claude.md",
            "adapters/browser.md",
            "host",
            "browser",
        ),
        (
            "contracts/source-use.md",
            "contracts/extra.md",
            "role",
            "contract",
        ),
    ] {
        let mut substituted: Value = serde_json::from_slice(&manifest).unwrap();
        let entry = substituted["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["path"] == original)
            .unwrap();
        entry["path"] = json!(replacement);
        entry[metadata_field] = json!(metadata_value);
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&substituted).unwrap(),
        )
        .unwrap();
        assert!(
            !run(&["--candidate-bundle", root.to_str().unwrap(),])
                .status
                .success(),
            "manifest must reject runtime-unconsumed path {replacement}"
        );
    }
    for (original, replacement) in [
        ("adapters/claude.md", "subagents/extra-adapter/SKILL.md"),
        (
            "contracts/source-use.md",
            "subagents/extra-contract/SKILL.md",
        ),
    ] {
        let mut substituted: Value = serde_json::from_slice(&manifest).unwrap();
        let entry = substituted["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["path"] == original)
            .unwrap();
        entry["path"] = json!(replacement);
        entry["role"] = json!("subagent");
        entry.as_object_mut().unwrap().remove("host");
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&substituted).unwrap(),
        )
        .unwrap();
        let output = run(&["--candidate-bundle", root.to_str().unwrap()]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unexpected learning bundle path"),
            "substituting {original} with {replacement} must fail the closed runtime inventory check: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::write(root.join("manifest.json"), &manifest).unwrap();

    for retired in [
        "subagents/source-researcher/SKILL.md",
        "subagents/task-critic/SKILL.md",
    ] {
        let path = root.join(retired);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "synthetic retired instruction asset\n").unwrap();
        let output = run(&["--candidate-bundle", root.to_str().unwrap()]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unrecognized files"));
        fs::remove_file(path).unwrap();
    }

    let mut retired_inventory: Value = serde_json::from_slice(&manifest).unwrap();
    for retired in [
        "subagents/source-researcher/SKILL.md",
        "subagents/task-critic/SKILL.md",
    ] {
        retired_inventory["files"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "path": retired,
                "role": "subagent",
                "required_for_activation": true,
                "sha256": hash(b"synthetic retired instruction asset\n"),
            }));
    }
    assert_eq!(retired_inventory["files"].as_array().unwrap().len(), 18);
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&retired_inventory).unwrap(),
    )
    .unwrap();
    let output = run(&["--candidate-bundle", root.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exactly 16 runtime-consumed files"));
    fs::write(root.join("manifest.json"), &manifest).unwrap();

    fs::remove_file(root.join("adapters/claude.md")).unwrap();
    assert!(
        !run(&["--candidate-bundle", root.to_str().unwrap(),])
            .status
            .success()
    );
    fs::write(
        root.join("adapters/claude.md"),
        b"synthetic gate fixture for adapters/claude.md\n",
    )
    .unwrap();

    let mut status_flip: Value = serde_json::from_slice(&manifest).unwrap();
    status_flip["status"] = json!("approved");
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&status_flip).unwrap(),
    )
    .unwrap();
    assert!(
        !run(&["--candidate-bundle", root.to_str().unwrap(),])
            .status
            .success()
    );
    let mut mixed_schema: Value = serde_json::from_slice(&manifest).unwrap();
    mixed_schema["minimum_learning_step_schema"] = json!(4);
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&mixed_schema).unwrap(),
    )
    .unwrap();
    assert!(
        !run(&["--candidate-bundle", root.to_str().unwrap(),])
            .status
            .success()
    );
    fs::write(root.join("manifest.json"), &manifest).unwrap();

    fs::write(
        root.join("MAGNESIUM.md"),
        b"changed without matching content digest",
    )
    .unwrap();
    assert!(
        !run(&["--candidate-bundle", root.to_str().unwrap()])
            .status
            .success()
    );

    let mut reserved: Value = serde_json::from_slice(&manifest).unwrap();
    reserved["files"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["path"] == "MAGNESIUM.md")
        .unwrap()["sha256"] = json!(hash(RESERVED));
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&reserved).unwrap(),
    )
    .unwrap();
    fs::write(root.join("MAGNESIUM.md"), RESERVED).unwrap();
    let output = run(&["--candidate-bundle", root.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("blank or reserved-marker content: MAGNESIUM.md"),
        "a matching digest must still not activate the reserved marker: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn release_report_cannot_promote_untested_behavior_or_include_test_assets() {
    let root = temp_path("report");
    fs::create_dir_all(&root).unwrap();
    let status = root.join("status.json");
    let current = fs::read(Path::new(ROOT).join("release/learning-status.json")).unwrap();
    let mut report: Value = serde_json::from_slice(&current).unwrap();
    report["actual_host_status"] = json!("PASS");
    fs::write(&status, serde_json::to_vec(&report).unwrap()).unwrap();
    let invalid_report = run(&["--source-root", ROOT, "--status", status.to_str().unwrap()]);
    assert!(!invalid_report.status.success());

    report["code_status"] = json!("POLICY_AUTHORED_AWAITING_LIVE_ACCEPTANCE");
    report["policy_status"] = json!("AUTHORED");
    report["actual_host_status"] = json!("FAIL");
    fs::write(&status, serde_json::to_vec(&report).unwrap()).unwrap();
    let failed_trial = run(&["--source-root", ROOT, "--status", status.to_str().unwrap()]);
    assert!(
        failed_trial.status.success(),
        "a failed live trial is reportable evidence: {}",
        String::from_utf8_lossy(&failed_trial.stderr)
    );

    report["code_status"] = json!("RELEASE_ELIGIBLE");
    report["actual_host_status"] = json!("FAIL");
    fs::write(&status, serde_json::to_vec(&report).unwrap()).unwrap();
    assert!(
        !run(&["--source-root", ROOT, "--status", status.to_str().unwrap()])
            .status
            .success()
    );

    report["actual_host_status"] = json!("PASS");
    fs::write(&status, serde_json::to_vec(&report).unwrap()).unwrap();
    let eligible_report = run(&["--source-root", ROOT, "--status", status.to_str().unwrap()]);
    assert!(
        eligible_report.status.success(),
        "eligible code with authored policy and native PASS is reportable while human learning remains unmeasured: {}",
        String::from_utf8_lossy(&eligible_report.stderr)
    );

    let archive = root.join("fixtures.zip");
    let script = format!(
        "import zipfile; z=zipfile.ZipFile(r'{}','w'); z.writestr('magnesium/tests/fixtures/example.txt','synthetic'); z.close()",
        archive.display()
    );
    assert!(
        Command::new("python3")
            .arg("-c")
            .arg(script)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        !run(&["--archive", archive.to_str().unwrap()])
            .status
            .success()
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn learning_archive_gate_accepts_zip_and_tar_and_rejects_synthetic_members() {
    let root = temp_path("tar-gate");
    fs::create_dir_all(&root).unwrap();
    let clean = root.join("clean.tar.gz");
    let synthetic = root.join("synthetic.tar.gz");
    let clean_zip = root.join("clean.zip");
    let script = r#"
import io, sys, tarfile
for archive_path, member in [
    (sys.argv[1], 'magnesium/MAGNESIUM.md'),
    (sys.argv[2], 'magnesium/tests/fixtures/example.txt'),
]:
    with tarfile.open(archive_path, 'w:gz') as archive:
        payload = b'fixture'
        info = tarfile.TarInfo(member)
        info.size = len(payload)
        archive.addfile(info, io.BytesIO(payload))
"#;
    assert!(
        Command::new("python3")
            .args(["-c", script])
            .arg(&clean)
            .arg(&synthetic)
            .status()
            .unwrap()
            .success()
    );
    let zip_script = "import sys, zipfile; z=zipfile.ZipFile(sys.argv[1], 'w'); z.writestr('magnesium/MAGNESIUM.md', 'policy'); z.close()";
    assert!(
        Command::new("python3")
            .args(["-c", zip_script])
            .arg(&clean_zip)
            .status()
            .unwrap()
            .success()
    );
    let accepted = run(&["--archive", clean.to_str().unwrap()]);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let accepted_zip = run(&["--archive", clean_zip.to_str().unwrap()]);
    assert!(
        accepted_zip.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted_zip.stderr)
    );
    assert!(
        !run(&["--archive", synthetic.to_str().unwrap()])
            .status
            .success()
    );
    let _ = fs::remove_dir_all(root);
}
