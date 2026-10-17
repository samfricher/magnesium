use std::{
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn git(args: &[&str]) -> Option<String> {
    let result = Command::new("git").args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}

fn read_reserved_file(name: &str) -> Vec<u8> {
    let path = Path::new(name);
    let mut checked = PathBuf::new();
    for component in path.components() {
        checked.push(component.as_os_str());
        let kind = fs::symlink_metadata(&checked)
            .expect("reserved learning instruction path")
            .file_type();
        assert!(
            !kind.is_symlink(),
            "reserved bundle path is a symlink: {name}"
        );
        if checked == path {
            assert!(kind.is_file(), "reserved bundle path is not a file: {name}");
        } else {
            assert!(
                kind.is_dir(),
                "reserved bundle parent is not a directory: {name}"
            );
        }
    }
    fs::read(path).expect("reserved learning instruction file")
}

fn reserved_inventory() -> Vec<(String, String, Option<String>, Option<String>)> {
    let bytes = read_reserved_file("src/workspace/learning/manifest.json");
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).expect("learning manifest JSON");
    assert_eq!(
        manifest.as_object().unwrap().len(),
        5,
        "closed learning manifest schema"
    );
    assert_eq!(manifest["bundle_version"], 2);
    assert!(matches!(
        manifest["status"].as_str(),
        Some("placeholder" | "active")
    ));
    assert_eq!(manifest["minimum_learning_step_schema"], 2);
    assert_eq!(manifest["entrypoint"], "MAGNESIUM.md");
    let entries = manifest["files"].as_array().expect("manifest files array");
    assert_eq!(
        entries.len(),
        16,
        "the reserved bundle has exactly 16 runtime-consumed files"
    );
    let mut inventory = Vec::with_capacity(entries.len());
    let mut seen = BTreeSet::new();
    let mut stages = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().expect("manifest file object");
        let path = entry["path"].as_str().expect("manifest path").to_owned();
        assert!(
            !path.is_empty()
                && !path.starts_with('/')
                && !path.contains('\\')
                && path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                && seen.insert(path.clone()),
            "unsafe or duplicate learning manifest path: {path}"
        );
        assert!(
            object.keys().all(|key| [
                "path",
                "role",
                "required_for_activation",
                "sha256",
                "stage",
                "host"
            ]
            .contains(&key.as_str())),
            "unknown manifest file metadata: {path}"
        );
        assert_eq!(
            entry["required_for_activation"], true,
            "all files are required: {path}"
        );
        let digest = entry["sha256"].as_str().expect("manifest sha256");
        assert_eq!(digest.len(), 64, "invalid manifest sha256: {path}");
        assert!(
            digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid manifest sha256: {path}"
        );
        let role = entry["role"].as_str().expect("manifest role").to_owned();
        let stage = entry
            .get("stage")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let host = entry
            .get("host")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let expected = if path == "MAGNESIUM.md" {
            ("orchestrator", None, None)
        } else if path == "teaching-policy.md" {
            ("policy", None, None)
        } else if path.starts_with("contracts/") {
            assert!(
                matches!(
                    path.as_str(),
                    "contracts/state-and-evidence.md"
                        | "contracts/source-use.md"
                        | "contracts/tool-recipes.md"
                ),
                "contract path has no runtime consumer: {path}"
            );
            ("contract", None, None)
        } else if path.starts_with("workflows/") {
            (
                "workflow",
                path.strip_prefix("workflows/")
                    .and_then(|name| name.strip_suffix(".md")),
                None,
            )
        } else if path.starts_with("adapters/") {
            assert!(
                matches!(
                    path.as_str(),
                    "adapters/codex.md" | "adapters/claude.md" | "adapters/opencode.md"
                ),
                "adapter path has no runtime consumer: {path}"
            );
            (
                "adapter",
                None,
                path.strip_prefix("adapters/")
                    .and_then(|name| name.strip_suffix(".md")),
            )
        } else {
            panic!("unexpected learning bundle path: {path}")
        };
        assert_eq!(role, expected.0, "wrong role: {path}");
        assert_eq!(stage.as_deref(), expected.1, "wrong stage: {path}");
        assert_eq!(host.as_deref(), expected.2, "wrong host: {path}");
        if let Some(stage) = &stage {
            stages.insert(stage.clone());
        }
        inventory.push((path, role, stage, host));
    }
    assert_eq!(
        stages,
        BTreeSet::from([
            "orient-and-probe".to_owned(),
            "plan-and-route".to_owned(),
            "teach-and-practice".to_owned(),
            "interpret-and-repair".to_owned(),
            "review-and-transfer".to_owned(),
            "consolidate-and-resume".to_owned(),
            "papers".to_owned(),
            "papers-loop".to_owned(),
        ]),
        "the learning manifest must include all required workflows"
    );
    let required_runtime_paths = BTreeSet::from([
        "MAGNESIUM.md",
        "teaching-policy.md",
        "contracts/state-and-evidence.md",
        "contracts/source-use.md",
        "contracts/tool-recipes.md",
        "workflows/orient-and-probe.md",
        "workflows/plan-and-route.md",
        "workflows/teach-and-practice.md",
        "workflows/interpret-and-repair.md",
        "workflows/review-and-transfer.md",
        "workflows/consolidate-and-resume.md",
        "workflows/papers.md",
        "workflows/papers-loop.md",
        "adapters/codex.md",
        "adapters/claude.md",
        "adapters/opencode.md",
    ]);
    let actual_runtime_paths: BTreeSet<_> = inventory
        .iter()
        .map(|(path, _, _, _)| path.as_str())
        .collect();
    assert_eq!(
        actual_runtime_paths, required_runtime_paths,
        "the manifest must contain exactly the paths consumed by the runtime"
    );
    inventory
}

fn main() {
    for path in [
        "src",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
        "codex-plugin",
        "claude-plugin",
        "opencode-plugin",
        "docs/user-guide.md",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    for reference in ["HEAD", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &reference])
    {
        println!("cargo:rerun-if-changed={path}");
    }
    let commit = git(&["rev-parse", "--verify", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = git(&["status", "--porcelain", "--untracked-files=normal"])
        .map(|status| if status.is_empty() { "false" } else { "true" })
        .unwrap_or("unknown");
    println!("cargo:rustc-env=MAGNESIUM_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=MAGNESIUM_BUILD_DIRTY={dirty}");
    let mut bytes = Vec::new();
    // Explicit runtime bundle members: never embed arbitrary untracked files,
    // credentials, build output or tests from a developer's plugin directory.
    for name in [
        "codex-plugin/.codex-plugin/plugin.json",
        "codex-plugin/.mcp.json",
        "codex-plugin/hooks/hooks.json",
        "codex-plugin/skills/learn/SKILL.md",
        "claude-plugin/.claude-plugin/plugin.json",
        "claude-plugin/.claude-plugin/marketplace.json",
        "claude-plugin/.mcp.json",
        "claude-plugin/hooks/hooks.json",
        "claude-plugin/skills/learn/SKILL.md",
        "claude-plugin/skills/papers/SKILL.md",
        "claude-plugin/skills/papers/templates/plan.md",
        "claude-plugin/skills/papers/templates/gap-log.md",
        "claude-plugin/skills/papers/templates/graph.yaml",
        "claude-plugin/skills/papers/templates/drills.md",
        "claude-plugin/skills/papers/templates/past-papers-README.md",
        "claude-plugin/skills/papers/templates/preamble.tex",
        "claude-plugin/skills/papers/templates/build.sh",
        "claude-plugin/skills/papers/templates/paper.md",
        "claude-plugin/skills/help/SKILL.md",
        "docs/user-guide.md",
        "opencode-plugin/magnesium.js",
        "opencode-plugin/skills/learn/SKILL.md",
    ] {
        let content = fs::read(name).expect("adapter bundle file");
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&(content.len() as u64).to_be_bytes());
        bytes.extend_from_slice(&content);
    }
    fs::write(
        Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR")).join("adapter-bundle.bin"),
        bytes,
    )
    .expect("write bundled adapter identity");

    // Package the active learner-model/v2 instructions reproducibly.
    let mut reserved = Vec::new();
    let inventory = reserved_inventory();
    let mut generated = String::from("const ALLOWED_PATHS: &[&str] = &[\n");
    for (relative, _, _, _) in &inventory {
        generated.push_str(&format!("    {relative:?},\n"));
    }
    generated.push_str(
        "];\nconst GENERATED_METADATA: &[(&str, &str, Option<&str>, Option<&str>)] = &[\n",
    );
    for (path, role, stage, host) in &inventory {
        generated.push_str(&format!(
            "    ({path:?}, {role:?}, {:?}, {:?}),\n",
            stage.as_deref(),
            host.as_deref()
        ));
    }
    generated.push_str("];\n");
    fs::write(
        Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR")).join("learning-manifest.rs"),
        generated,
    )
    .expect("write generated learning manifest metadata");
    for relative in
        std::iter::once("manifest.json".to_owned()).chain(inventory.into_iter().map(|row| row.0))
    {
        let name = format!("src/workspace/learning/{relative}");
        let content = read_reserved_file(&name);
        reserved.extend_from_slice(relative.as_bytes());
        reserved.push(0);
        reserved.extend_from_slice(&(content.len() as u64).to_be_bytes());
        reserved.extend_from_slice(&content);
    }
    fs::write(
        Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR")).join("learning-instructions-v2.bin"),
        reserved,
    )
    .expect("write reserved learning instruction bundle");
}
