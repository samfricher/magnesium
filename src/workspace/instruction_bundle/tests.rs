#[test]
fn unknown_bundle_directory_fails_at_the_first_unauthorized_entry() {
    let bundle = Arc::new(synthetic_bundle());
    let control = temp_control("bundle-unknown-directory");
    install_known_bundle(&control, &bundle).unwrap();
    let instructions = control.join("instructions");
    fs::create_dir_all(
        instructions
            .join("v2")
            .join(&bundle.hash)
            .join("nested/empty"),
    )
    .unwrap();
    assert!(
        matches!(
            load_installed(&instructions, &bundle),
            Err(BundleError::UnsafePath)
        ),
        "an unknown empty directory must fail before any traversal"
    );
    fs::remove_dir_all(control).unwrap();
}
use super::{
    ALLOWED_PATHS, Arc, BundleError, InstructionHost, InstructionOperation, InstructionStage,
    MANIFEST, compiled_bundle, error_code, install_bundle, install_known_bundle, load_inner,
    load_installed, render, safe_relative_path, validate_bundle, validate_installed_dir,
    write_bundle_files,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

fn temp_control(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "magnesium-instruction-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("control directory");
    fs::canonicalize(path).expect("canonical control directory")
}

const STAGES: [InstructionStage; 8] = [
    InstructionStage::OrientAndProbe,
    InstructionStage::PlanAndRoute,
    InstructionStage::TeachAndPractice,
    InstructionStage::InterpretAndRepair,
    InstructionStage::ReviewAndTransfer,
    InstructionStage::ConsolidateAndResume,
    InstructionStage::Papers,
    InstructionStage::PapersLoop,
];

#[test]
fn every_stage_selects_its_manifest_workflow() {
    for stage in STAGES {
        assert!(
            super::GENERATED_METADATA
                .iter()
                .any(|(path, role, metadata_stage, _)| {
                    *role == "workflow"
                        && *metadata_stage == Some(stage.manifest_key())
                        && *path == stage.workflow()
                })
        );
    }
}

#[test]
fn compiled_approved_bundle_activates_and_serves() {
    let bundle = compiled_bundle().expect("compiled approved assets validate");
    assert_eq!(bundle.status, "active");
    assert_eq!(bundle.files.len(), 15);
    let control = temp_control("approved");
    assert_eq!(
        install_bundle(&control).expect("approved install"),
        super::InstallDisposition::CreatedAndActivated
    );
    assert!(
        control
            .join("instructions/active.json")
            .try_exists()
            .unwrap()
    );
    let loaded = load_inner(
        &control,
        InstructionStage::TeachAndPractice,
        InstructionHost::Claude,
        None,
        "full_adapter",
    )
    .expect("approved instruction load");
    assert_eq!(loaded.status, "served");
    assert_eq!(loaded.bundle_hash, bundle.hash);
    assert!(loaded.immutable_bytes_reused);
    assert_eq!(loaded.validation_file_count, 16);
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn manifest_paths_are_closed_and_reject_traversal() {
    for allowed in ALLOWED_PATHS {
        assert!(safe_relative_path(allowed));
    }
    for unsafe_path in [
        "../secret",
        "workflows/../../secret",
        "/absolute",
        "adapters\\claude.md",
    ] {
        assert!(!safe_relative_path(unsafe_path));
    }
    assert_eq!(error_code(&BundleError::Placeholder), "placeholder_bundle");
}

#[test]
fn operation_contracts_select_only_the_requested_module() {
    assert_eq!(
        InstructionOperation::StateAndEvidence.asset(),
        "contracts/state-and-evidence.md"
    );
    assert_eq!(
        InstructionOperation::SourceUse.asset(),
        "contracts/source-use.md"
    );
    assert_eq!(
        InstructionOperation::ToolRecipes.asset(),
        "contracts/tool-recipes.md"
    );
}

#[test]
fn every_runtime_host_and_operation_has_a_manifest_consumer() {
    let host = InstructionHost::Claude;
    assert!(
        super::GENERATED_METADATA
            .iter()
            .any(|(path, role, _, metadata_host)| {
                *path == host.asset()
                    && *role == "adapter"
                    && *metadata_host == Some(host.manifest_key())
            })
    );

    for operation in [
        InstructionOperation::StateAndEvidence,
        InstructionOperation::SourceUse,
        InstructionOperation::ToolRecipes,
    ] {
        let path = operation.asset();
        assert!(
            super::GENERATED_METADATA
                .iter()
                .any(|(entry_path, role, _, _)| { *entry_path == path && *role == "contract" })
        );
    }
}

/// Skills such as papers need tool_recipes on every turn, so a workflow
/// that grows past the remaining budget must fail here, not defer recipes.
#[test]
fn every_stage_serves_tool_recipes_without_deferral() {
    let bundle = compiled_bundle().unwrap();
    let control = temp_control("tool-recipes-budget");
    install_bundle(&control).unwrap();
    for stage in STAGES {
        let host = InstructionHost::Claude;
        let loaded = load_inner(
            &control,
            stage,
            host,
            Some(InstructionOperation::ToolRecipes),
            "instruction_only",
        )
        .unwrap();
        let needed = loaded.bytes_served
            + loaded
                .deferred_modules
                .iter()
                .map(|path| bundle.files[path].len() + 2)
                .sum::<usize>();
        assert!(
            loaded.deferred_modules.is_empty(),
            "{stage:?} for {host:?} needs {needed} of {} bytes, {} over, so it deferred {:?}",
            super::RESPONSE_LIMIT,
            needed.saturating_sub(super::RESPONSE_LIMIT),
            loaded.deferred_modules
        );
    }
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn every_packaged_instruction_has_a_supported_load_route() {
    let bundle = compiled_bundle().unwrap();
    let control = temp_control("all-load-routes");
    install_bundle(&control).unwrap();
    let validation_bytes = bundle.files.values().map(Vec::len).sum::<usize>()
        + fs::read(control.join("instructions/active.json"))
            .unwrap()
            .len();
    let mut served = std::collections::BTreeSet::new();
    for stage in STAGES {
        let host = InstructionHost::Claude;
        for operation in [
            None,
            Some(InstructionOperation::StateAndEvidence),
            Some(InstructionOperation::SourceUse),
            Some(InstructionOperation::ToolRecipes),
        ] {
            let loaded = load_inner(&control, stage, host, operation, "instruction_only").unwrap();
            let mut expected_files = vec![
                "MAGNESIUM.md",
                "teaching-policy.md",
                host.asset(),
                stage.workflow(),
            ];
            if let Some(operation) = operation {
                expected_files.push(operation.asset());
            }
            assert_eq!(loaded.files, expected_files);
            assert!(loaded.deferred_modules.is_empty());
            let expected_text = expected_files
                .iter()
                .map(|path| std::str::from_utf8(&bundle.files[*path]).unwrap())
                .collect::<Vec<_>>()
                .join("\n\n");
            assert_eq!(loaded.text, expected_text);
            assert_eq!(loaded.status, "served");
            assert_eq!(loaded.bundle_version, bundle.version);
            assert_eq!(loaded.bundle_hash, bundle.hash);
            assert_eq!(loaded.recommended_stage, stage);
            assert_eq!(loaded.bytes_served, expected_text.len());
            assert!(loaded.bytes_served <= super::RESPONSE_LIMIT);
            assert_eq!(loaded.validation_file_count, 16);
            assert_eq!(loaded.validation_bytes, validation_bytes);
            assert!(loaded.immutable_bytes_reused);
            let response = super::load(&control, stage, host, operation, "instruction_only");
            assert_eq!(response, serde_json::to_value(&loaded).unwrap());
            assert_eq!(
                response
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                [
                    "bundle_hash",
                    "bundle_version",
                    "bytes_served",
                    "capability_tier",
                    "deferred_modules",
                    "files",
                    "immutable_bytes_reused",
                    "recommended_stage",
                    "status",
                    "text",
                    "validation_bytes",
                    "validation_file_count",
                ]
            );
            served.extend(loaded.files);
        }
    }
    assert_eq!(served.len(), 14);
    assert_eq!(
        served,
        ALLOWED_PATHS
            .iter()
            .map(|path| (*path).to_owned())
            .collect()
    );
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn unavailable_instruction_routes_return_identity_and_actionable_error_only() {
    let bundle = compiled_bundle().unwrap();
    let control = temp_control("unavailable-routes");
    let instructions = control.join("instructions");
    fs::create_dir(&instructions).unwrap();
    for stage in STAGES {
        let host = InstructionHost::Claude;
        for operation in [
            None,
            Some(InstructionOperation::StateAndEvidence),
            Some(InstructionOperation::SourceUse),
            Some(InstructionOperation::ToolRecipes),
        ] {
            let response = super::load(&control, stage, host, operation, "instruction_only");
            assert_eq!(response["status"], "unavailable");
            assert_eq!(response["bundle_version"], bundle.version);
            assert_eq!(response["bundle_hash"], bundle.hash);
            assert_eq!(
                response["recommended_stage"],
                serde_json::to_value(stage).unwrap()
            );
            assert_eq!(response["error"]["code"], "bundle_not_active");
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("run `magnesium init --upgrade-instructions` in this workspace")
            );
            assert_eq!(
                response
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                [
                    "bundle_hash",
                    "bundle_version",
                    "error",
                    "recommended_stage",
                    "status"
                ]
            );
        }
    }
    assert_eq!(fs::read_dir(&control).unwrap().count(), 1);
    assert_eq!(fs::read_dir(&instructions).unwrap().count(), 0);
    fs::remove_dir_all(control).unwrap();
}

fn synthetic_bundle() -> super::Bundle {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/instruction_bundle");
    let mut files = BTreeMap::new();
    for relative in std::iter::once("manifest.json").chain(ALLOWED_PATHS.iter().copied()) {
        files.insert(
            relative.to_owned(),
            fs::read(root.join(relative)).expect("synthetic fixture"),
        );
    }
    let manifest = files.get("manifest.json").unwrap().clone();
    validate_bundle(manifest, files).expect("synthetic reviewed fixture")
}

fn revised_synthetic_bundle() -> super::Bundle {
    let base = synthetic_bundle();
    let mut files = (*base.files).clone();
    files.insert(
        "MAGNESIUM.md".to_owned(),
        b"SYNTHETIC_FIXTURE: revision B\n".to_vec(),
    );
    let mut manifest: Value = serde_json::from_slice(&base.files[MANIFEST]).unwrap();
    manifest["files"][0]["sha256"] = Value::from(super::sha256(&files["MAGNESIUM.md"]));
    let manifest = serde_json::to_vec(&manifest).unwrap();
    files.insert(MANIFEST.to_owned(), manifest.clone());
    validate_bundle(manifest, files).expect("revised synthetic bundle")
}

#[test]
fn cycle_activation_requires_matching_installed_bundle_at_current_pointer() {
    let control = temp_control("cycle-capability");
    let bundle = synthetic_bundle();
    install_known_bundle(&control, &bundle).unwrap();
    assert!(super::load_installed(&control.join("instructions"), &bundle).is_ok());

    let pointer = control.join("instructions/active.json");
    let original = fs::read(&pointer).unwrap();
    let changed = super::ActivePointer {
        bundle_version: bundle.version,
        bundle_hash: "0".repeat(64),
    };
    fs::write(&pointer, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(super::load_installed(&control.join("instructions"), &bundle).is_err());
    fs::remove_file(&pointer).unwrap();
    assert!(super::load_installed(&control.join("instructions"), &bundle).is_err());
    fs::write(&pointer, original).unwrap();
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn repeated_bundle_loads_revalidate_disk_and_serve_compiled_bytes() {
    let bundle = synthetic_bundle();
    let control = temp_control("stage-sequence-cache");
    install_known_bundle(&control, &bundle).unwrap();
    let instructions = control.join("instructions");
    let first_validation = load_installed(&instructions, &bundle).unwrap();
    let second_validation = load_installed(&instructions, &bundle).unwrap();
    assert_eq!(first_validation.file_count, 16);
    assert_eq!(
        first_validation.bytes,
        bundle.files.values().map(Vec::len).sum::<usize>()
            + fs::read(instructions.join("active.json")).unwrap().len()
    );
    assert_eq!(first_validation.bytes, second_validation.bytes);

    for (sequence_index, stage) in STAGES.into_iter().enumerate() {
        let response = render(
            &bundle,
            stage,
            InstructionHost::Claude,
            None,
            "instruction_only",
            second_validation,
            false,
        )
        .unwrap();
        assert_eq!(response.recommended_stage, stage);
        assert_eq!(
            response.files,
            [
                "MAGNESIUM.md",
                "teaching-policy.md",
                "adapters/claude.md",
                stage.workflow(),
            ]
        );
        assert_eq!(
            response
                .files
                .iter()
                .filter(|path| path.starts_with("workflows/"))
                .count(),
            1
        );
        assert!(
            response
                .text
                .contains(&format!("SYNTHETIC_FIXTURE: {}", stage.workflow()))
        );
        assert!(response.bytes_served <= super::RESPONSE_LIMIT);
        assert!(
            !response.immutable_bytes_reused,
            "stage sequence item {sequence_index}"
        );
        assert_eq!(response.validation_file_count, 16);
        assert_eq!(response.validation_bytes, second_validation.bytes);
    }

    let claude = render(
        &bundle,
        InstructionStage::InterpretAndRepair,
        InstructionHost::Claude,
        None,
        "instruction_only",
        second_validation,
        false,
    )
    .unwrap();
    assert_eq!(claude.files[2], "adapters/claude.md");
    assert!(!claude.immutable_bytes_reused);
    fs::write(
        instructions
            .join("v2")
            .join(&bundle.hash)
            .join("MAGNESIUM.md"),
        b"local edit after cache hit\n",
    )
    .unwrap();
    assert!(matches!(
        load_installed(&instructions, &bundle),
        Err(BundleError::UserEdits)
    ));
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn one_hundred_workspace_loads_share_immutable_bundle_storage() {
    let bundle = compiled_bundle().unwrap();
    let mut validation_bytes_per_workspace = None;
    let mut total_validation_bytes = 0usize;
    for index in 0..100 {
        let control = temp_control(&format!("bundle-workspace-{index}"));
        install_known_bundle(&control, &bundle).unwrap();
        let loaded = load_inner(
            &control,
            InstructionStage::TeachAndPractice,
            InstructionHost::Claude,
            None,
            "instruction_only",
        )
        .unwrap();
        let loaded_bundle = compiled_bundle().unwrap();
        assert!(Arc::ptr_eq(&loaded_bundle, &bundle));
        assert!(Arc::ptr_eq(&loaded_bundle.files, &bundle.files));
        assert!(loaded.immutable_bytes_reused);
        assert_eq!(loaded.bundle_hash, bundle.hash);
        // One active-pointer read plus the 14 instructions and manifest; the
        // ordinary path must not reopen the pointer it already verified.
        assert_eq!(loaded.validation_file_count, 16);
        match validation_bytes_per_workspace {
            Some(bytes) => assert_eq!(loaded.validation_bytes, bytes),
            None => validation_bytes_per_workspace = Some(loaded.validation_bytes),
        }
        total_validation_bytes += loaded.validation_bytes;
        fs::remove_dir_all(control).unwrap();
    }
    // Full local validation remains uncached; immutable bundle storage is
    // shared even as every workspace's installed files are reread.
    assert_eq!(
        total_validation_bytes,
        validation_bytes_per_workspace.unwrap() * 100
    );
    assert_eq!(Arc::strong_count(&bundle.files), 1);
}

#[test]
fn synthetic_optional_contract_is_loaded_only_for_its_operation() {
    let bundle = synthetic_bundle();
    let validation = super::ValidationWork {
        file_count: 15,
        bytes: 0,
    };
    let no_operation = render(
        &bundle,
        InstructionStage::TeachAndPractice,
        InstructionHost::Claude,
        None,
        "instruction_only",
        validation,
        false,
    )
    .unwrap();
    let with_operation = render(
        &bundle,
        InstructionStage::TeachAndPractice,
        InstructionHost::Claude,
        Some(InstructionOperation::SourceUse),
        "instruction_only",
        validation,
        false,
    )
    .unwrap();
    assert!(
        !no_operation
            .files
            .iter()
            .any(|file| file.starts_with("contracts/"))
    );
    assert!(
        with_operation
            .files
            .contains(&"contracts/source-use.md".to_owned())
    );
    assert_eq!(
        with_operation
            .files
            .iter()
            .filter(|file| file.starts_with("workflows/"))
            .count(),
        1
    );
}

#[test]
fn malformed_version_hash_and_manifest_paths_fail_closed() {
    let bundle = synthetic_bundle();
    let manifest_bytes = bundle.files[MANIFEST].clone();
    let mut broken_files = (*bundle.files).clone();
    broken_files.insert(
        "teaching-policy.md".to_owned(),
        b"changed fixture\n".to_vec(),
    );
    assert!(matches!(
        validate_bundle(manifest_bytes.clone(), broken_files),
        Err(BundleError::FileMismatch)
    ));

    for (original, replacement) in [
        ("adapters/claude.md", "adapters/browser.md"),
        ("contracts/source-use.md", "contracts/extra.md"),
    ] {
        let mut substituted: Value = serde_json::from_slice(&manifest_bytes).unwrap();
        let entry = substituted["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["path"] == original)
            .unwrap();
        entry["path"] = Value::from(replacement);
        let substituted = serde_json::to_vec(&substituted).unwrap();
        let mut files = (*bundle.files).clone();
        files.insert(MANIFEST.to_owned(), substituted.clone());
        assert!(matches!(
            validate_bundle(substituted, files),
            Err(BundleError::UnsafePath)
        ));
    }

    for (original, replacement) in [
        ("adapters/claude.md", "subagents/extra-adapter/SKILL.md"),
        (
            "contracts/source-use.md",
            "subagents/extra-contract/SKILL.md",
        ),
    ] {
        let mut substituted: Value = serde_json::from_slice(&manifest_bytes).unwrap();
        let entry = substituted["files"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["path"] == original)
            .unwrap();
        entry["path"] = Value::from(replacement);
        entry["role"] = Value::from("subagent");
        entry.as_object_mut().unwrap().remove("host");
        let substituted = serde_json::to_vec(&substituted).unwrap();
        let mut files = (*bundle.files).clone();
        files.insert(MANIFEST.to_owned(), substituted.clone());
        assert!(
            matches!(
                validate_bundle(substituted, files),
                Err(BundleError::UnsafePath)
            ),
            "runtime must reject a subagent replacing {original}"
        );
    }

    let mut wrong_role: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    wrong_role["files"][0]["role"] = Value::from("policy");
    let wrong_role = serde_json::to_vec(&wrong_role).unwrap();
    let mut files = (*bundle.files).clone();
    files.insert(MANIFEST.to_owned(), wrong_role.clone());
    assert!(matches!(
        validate_bundle(wrong_role, files),
        Err(BundleError::FileMismatch)
    ));

    let mut value: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    value["bundle_version"] = Value::from(99);
    let manifest = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        validate_bundle(manifest, (*bundle.files).clone()),
        Err(BundleError::UnsupportedBundle)
    ));

    value["bundle_version"] = Value::from(2);
    value["files"][0]["path"] = Value::from("../outside.md");
    let manifest = serde_json::to_vec(&value).unwrap();
    let mut files = (*bundle.files).clone();
    files.insert("manifest.json".to_owned(), manifest.clone());
    assert!(matches!(
        validate_bundle(manifest, files),
        Err(BundleError::UnsafePath)
    ));

    let mut value: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    value["minimum_learning_step_schema"] = Value::from(999);
    let manifest = serde_json::to_vec(&value).unwrap();
    let mut files = (*bundle.files).clone();
    files.insert("manifest.json".to_owned(), manifest.clone());
    assert!(matches!(
        validate_bundle(manifest, files),
        Err(BundleError::UnsupportedSchema)
    ));

    let mut files = (*bundle.files).clone();
    files.remove("workflows/teach-and-practice.md");
    assert!(matches!(
        validate_bundle(manifest_bytes.clone(), files),
        Err(BundleError::ManifestMismatch)
    ));

    let mut files = (*bundle.files).clone();
    files.insert("unexpected.md".to_owned(), b"unexpected".to_vec());
    assert!(matches!(
        validate_bundle(manifest_bytes, files),
        Err(BundleError::ManifestMismatch)
    ));
}

#[test]
fn oversized_required_core_fails_without_truncation_and_optional_module_is_deferred() {
    let mut bundle = synthetic_bundle();
    Arc::make_mut(&mut bundle.files)
        .insert("MAGNESIUM.md".to_owned(), vec![b'x'; super::RESPONSE_LIMIT]);
    assert!(matches!(
        render(
            &bundle,
            InstructionStage::TeachAndPractice,
            InstructionHost::Claude,
            None,
            "full_adapter",
            super::ValidationWork {
                file_count: 15,
                bytes: 0
            },
            false,
        ),
        Err(BundleError::RequiredSetTooLarge)
    ));

    let mut bundle = synthetic_bundle();
    Arc::make_mut(&mut bundle.files).insert(
        "contracts/tool-recipes.md".to_owned(),
        vec![b'x'; super::RESPONSE_LIMIT],
    );
    let output = render(
        &bundle,
        InstructionStage::TeachAndPractice,
        InstructionHost::Claude,
        Some(InstructionOperation::ToolRecipes),
        "tool_only",
        super::ValidationWork {
            file_count: 15,
            bytes: 0,
        },
        false,
    )
    .unwrap();
    assert!(
        output
            .deferred_modules
            .contains(&"contracts/tool-recipes.md".to_owned())
    );
    assert!(
        !output
            .files
            .contains(&"contracts/tool-recipes.md".to_owned())
    );
    assert!(output.text.len() <= super::RESPONSE_LIMIT);
}

#[test]
fn failed_or_unrecognized_update_preserves_existing_bundle_and_pointer() {
    let control = temp_control("failed-update");
    let bundle = synthetic_bundle();
    assert_eq!(
        install_known_bundle(&control, &bundle).unwrap(),
        super::InstallDisposition::CreatedAndActivated
    );
    let instructions = control.join("instructions");
    let pointer_before = fs::read(instructions.join("active.json")).unwrap();
    let installed = instructions.join("v2").join(&bundle.hash);
    fs::write(installed.join("MAGNESIUM.md"), b"user edit\n").unwrap();
    assert!(matches!(
        install_known_bundle(&control, &bundle),
        Err(BundleError::UserEdits)
    ));
    assert_eq!(
        fs::read(instructions.join("active.json")).unwrap(),
        pointer_before
    );
    assert_eq!(
        fs::read(installed.join("MAGNESIUM.md")).unwrap(),
        b"user edit\n"
    );
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn retired_subagent_asset_bundle_requires_fresh_instruction_setup() {
    let current = synthetic_bundle();
    let mut old_files = (*current.files).clone();
    let mut old_manifest: Value = serde_json::from_slice(&current.files[MANIFEST]).unwrap();
    for path in [
        "subagents/source-researcher/SKILL.md",
        "subagents/task-critic/SKILL.md",
    ] {
        let bytes = format!("SYNTHETIC_RETIRED_ASSET: {path}\n").into_bytes();
        old_manifest["files"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "path": path,
                "role": "subagent",
                "required_for_activation": true,
                "sha256": super::sha256(&bytes),
            }));
        old_files.insert(path.to_owned(), bytes);
    }
    let old_manifest_bytes = serde_json::to_vec(&old_manifest).unwrap();
    old_files.insert(MANIFEST.to_owned(), old_manifest_bytes.clone());
    assert_eq!(old_manifest["files"].as_array().unwrap().len(), 16);
    assert!(matches!(
        validate_bundle(old_manifest_bytes.clone(), old_files.clone()),
        Err(BundleError::ManifestMismatch)
    ));
    let old_manifest: super::Manifest = serde_json::from_slice(&old_manifest_bytes).unwrap();
    let old_hash = super::bundle_hash(&old_manifest, &old_manifest_bytes);
    assert_ne!(old_hash, current.hash);
    let old_bundle = super::Bundle {
        version: old_manifest.bundle_version,
        hash: old_hash.clone(),
        status: old_manifest.status,
        files: Arc::new(old_files),
    };
    let old_control = temp_control("retired-inventory");
    let old_directory = old_control.join("instructions/v2").join(&old_hash);
    fs::create_dir_all(&old_directory).unwrap();
    write_bundle_files(&old_directory, &old_bundle).unwrap();
    let pointer_path = old_control.join("instructions/active.json");
    let old_pointer = serde_json::to_vec(&super::ActivePointer {
        bundle_version: old_bundle.version,
        bundle_hash: old_hash.clone(),
    })
    .unwrap();
    fs::write(&pointer_path, &old_pointer).unwrap();
    assert!(matches!(
        install_known_bundle(&old_control, &current),
        Err(BundleError::PreviousBundleActive)
    ));
    assert!(matches!(
        load_installed(&old_control.join("instructions"), &current),
        Err(BundleError::ActivePointer)
    ));
    assert_eq!(fs::read(&pointer_path).unwrap(), old_pointer);
    for (path, bytes) in old_bundle.files.iter() {
        assert_eq!(&fs::read(old_directory.join(path)).unwrap(), bytes);
    }

    assert_eq!(
        super::upgrade_known_bundle(&old_control, &current).unwrap(),
        super::InstallDisposition::Upgraded {
            previous: old_hash.clone()
        }
    );
    assert!(load_installed(&old_control.join("instructions"), &current).is_ok());
    for (path, bytes) in old_bundle.files.iter() {
        assert_eq!(&fs::read(old_directory.join(path)).unwrap(), bytes);
    }

    let fresh_control = temp_control("fresh-inventory");
    assert_eq!(
        install_known_bundle(&fresh_control, &current).unwrap(),
        super::InstallDisposition::CreatedAndActivated
    );
    assert_eq!(
        install_known_bundle(&fresh_control, &current).unwrap(),
        super::InstallDisposition::AlreadyActive
    );
    let fresh_validation = load_installed(&fresh_control.join("instructions"), &current).unwrap();
    assert_eq!(fresh_validation.file_count, 16);
    assert_eq!(current.files.len(), 15);
    assert_eq!(current.version, 2);
    fs::remove_dir_all(old_control).unwrap();
    fs::remove_dir_all(fresh_control).unwrap();
}

#[test]
fn an_earlier_active_bundle_is_seen_without_a_change() {
    let control = temp_control("earlier-bundle-seen");
    let old = synthetic_bundle();
    let current = revised_synthetic_bundle();
    assert!(!super::earlier_known_bundle_active(&control, &current));
    install_known_bundle(&control, &old).unwrap();
    let pointer = control.join("instructions/active.json");
    let before = fs::read(&pointer).unwrap();
    assert!(super::earlier_known_bundle_active(&control, &current));
    assert_eq!(
        fs::read(&pointer).unwrap(),
        before,
        "looking changes nothing"
    );
    assert!(
        !control.join("instructions/v2").join(&current.hash).exists(),
        "looking stages nothing"
    );
    assert!(!super::earlier_known_bundle_active(&control, &old));
    // An edited pointer is not an earlier version's, and the upgrade
    // would refuse it, so it is not reported as one.
    fs::write(&pointer, b"{\"bundle_version\":2, \"bundle_hash\":\"x\"}").unwrap();
    assert!(!super::earlier_known_bundle_active(&control, &current));
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn changed_policy_hash_is_staged_but_never_replaces_or_serves_an_old_pointer() {
    let control = temp_control("changed-policy-rejected");
    let old = synthetic_bundle();
    let current = revised_synthetic_bundle();
    assert_ne!(old.hash, current.hash);
    assert_eq!(old.files.len(), current.files.len());
    install_known_bundle(&control, &old).unwrap();
    let instructions = control.join("instructions");
    let pointer_before = fs::read(instructions.join("active.json")).unwrap();
    let old_directory = instructions.join("v2").join(&old.hash);
    let current_directory = instructions.join("v2").join(&current.hash);

    for _ in 0..2 {
        assert!(matches!(
            install_known_bundle(&control, &current),
            Err(BundleError::PreviousBundleActive)
        ));
        assert!(matches!(
            load_installed(&instructions, &current),
            Err(BundleError::ActivePointer)
        ));
        assert_eq!(
            fs::read(instructions.join("active.json")).unwrap(),
            pointer_before
        );
        for (path, bytes) in old.files.iter() {
            assert_eq!(&fs::read(old_directory.join(path)).unwrap(), bytes);
        }
        for (path, bytes) in current.files.iter() {
            assert_eq!(&fs::read(current_directory.join(path)).unwrap(), bytes);
        }
        assert!(load_installed(&instructions, &old).is_ok());
    }
    assert_eq!(fs::read_dir(instructions.join("v2")).unwrap().count(), 2);
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn explicit_upgrade_moves_the_pointer_from_an_unedited_previous_bundle_only() {
    let control = temp_control("explicit-upgrade");
    let old = synthetic_bundle();
    let current = revised_synthetic_bundle();
    install_known_bundle(&control, &old).unwrap();
    let instructions = control.join("instructions");
    let old_directory = instructions.join("v2").join(&old.hash);
    assert_eq!(
        super::upgrade_known_bundle(&control, &current).unwrap(),
        super::InstallDisposition::Upgraded {
            previous: old.hash.clone()
        }
    );
    assert!(load_installed(&instructions, &current).is_ok());
    assert!(matches!(
        load_installed(&instructions, &old),
        Err(BundleError::ActivePointer)
    ));
    for (path, bytes) in old.files.iter() {
        assert_eq!(&fs::read(old_directory.join(path)).unwrap(), bytes);
    }
    assert_eq!(
        super::upgrade_known_bundle(&control, &current).unwrap(),
        super::InstallDisposition::AlreadyActive
    );
    fs::remove_dir_all(control).unwrap();

    // Local edits to the earlier bundle keep it active and untouched.
    for edit in ["MAGNESIUM.md", "extra.md"] {
        let control = temp_control("edited-upgrade");
        install_known_bundle(&control, &old).unwrap();
        let instructions = control.join("instructions");
        let pointer_before = fs::read(instructions.join("active.json")).unwrap();
        let edited = instructions.join("v2").join(&old.hash).join(edit);
        fs::write(&edited, b"my own note\n").unwrap();
        assert!(matches!(
            super::upgrade_known_bundle(&control, &current),
            Err(BundleError::UserEdits)
        ));
        assert_eq!(
            fs::read(instructions.join("active.json")).unwrap(),
            pointer_before
        );
        assert_eq!(fs::read(&edited).unwrap(), b"my own note\n");
        assert!(!instructions.join("v2").join(&current.hash).exists());
        fs::remove_dir_all(control).unwrap();
    }
}

#[test]
fn edited_current_pointer_is_rejected_without_replacement() {
    let control = temp_control("edited-current-pointer");
    let bundle = synthetic_bundle();
    install_known_bundle(&control, &bundle).unwrap();
    let instructions = control.join("instructions");
    let pointer_path = instructions.join("active.json");
    let original = fs::read(&pointer_path).unwrap();
    let mut noncanonical = original.clone();
    noncanonical.push(b' ');
    let mut wrong_version: Value = serde_json::from_slice(&original).unwrap();
    wrong_version["bundle_version"] = Value::from(99);
    let mut malformed_hash: Value = serde_json::from_slice(&original).unwrap();
    malformed_hash["bundle_hash"] = Value::from("../outside");
    let mut extra_field: Value = serde_json::from_slice(&original).unwrap();
    extra_field["allow_previous"] = Value::from(true);
    for edited in [
        noncanonical,
        serde_json::to_vec(&wrong_version).unwrap(),
        serde_json::to_vec(&malformed_hash).unwrap(),
        serde_json::to_vec(&extra_field).unwrap(),
    ] {
        fs::write(&pointer_path, &edited).unwrap();
        assert!(install_known_bundle(&control, &bundle).is_err());
        assert!(load_installed(&instructions, &bundle).is_err());
        assert_eq!(fs::read(&pointer_path).unwrap(), edited);
    }
    fs::write(&pointer_path, &original).unwrap();
    assert_eq!(
        install_known_bundle(&control, &bundle).unwrap(),
        super::InstallDisposition::AlreadyActive
    );
    assert_eq!(
        load_installed(&instructions, &bundle).unwrap().file_count,
        16
    );
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn current_staged_bundle_is_validated_before_recreating_a_missing_pointer() {
    let control = temp_control("current-staged-pointer");
    let bundle = synthetic_bundle();
    install_known_bundle(&control, &bundle).unwrap();
    let instructions = control.join("instructions");
    let pointer_path = instructions.join("active.json");
    let original = fs::read(&pointer_path).unwrap();
    fs::remove_file(&pointer_path).unwrap();
    assert_eq!(
        install_known_bundle(&control, &bundle).unwrap(),
        super::InstallDisposition::UpdatedActive
    );
    assert_eq!(fs::read(&pointer_path).unwrap(), original);
    assert_eq!(
        load_installed(&instructions, &bundle).unwrap().file_count,
        16
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&pointer_path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    fs::remove_file(&pointer_path).unwrap();
    let edited = instructions
        .join("v2")
        .join(&bundle.hash)
        .join("MAGNESIUM.md");
    fs::write(&edited, b"edited staged policy\n").unwrap();
    assert!(matches!(
        install_known_bundle(&control, &bundle),
        Err(BundleError::UserEdits)
    ));
    assert!(!pointer_path.exists());
    assert_eq!(fs::read(&edited).unwrap(), b"edited staged policy\n");
    fs::remove_dir_all(control).unwrap();
}

#[cfg(unix)]
#[test]
fn installed_symlink_is_rejected() {
    use std::os::unix::fs::symlink;
    let control = temp_control("symlink");
    let bundle = synthetic_bundle();
    install_known_bundle(&control, &bundle).unwrap();
    let installed = control.join("instructions/v2").join(&bundle.hash);
    fs::remove_file(installed.join("MAGNESIUM.md")).unwrap();
    let outside = control.join("outside.md");
    fs::write(&outside, b"outside\n").unwrap();
    symlink(&outside, installed.join("MAGNESIUM.md")).unwrap();
    assert!(matches!(
        validate_installed_dir(&installed, &bundle),
        Err(BundleError::Symlink)
    ));
    fs::remove_dir_all(control).unwrap();
}

#[cfg(unix)]
#[test]
fn active_pointer_symlink_is_rejected_without_replacement() {
    use std::os::unix::fs::symlink;
    let control = temp_control("pointer-symlink");
    let bundle = synthetic_bundle();
    install_known_bundle(&control, &bundle).unwrap();
    let instructions = control.join("instructions");
    let pointer = instructions.join("active.json");
    let outside = control.join("outside.json");
    fs::write(&outside, fs::read(&pointer).unwrap()).unwrap();
    fs::remove_file(&pointer).unwrap();
    symlink(&outside, &pointer).unwrap();
    assert!(matches!(
        install_known_bundle(&control, &bundle),
        Err(BundleError::Symlink)
    ));
    assert!(
        fs::symlink_metadata(&pointer)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn installed_file_growth_is_rejected_after_a_bounded_read() {
    let control = temp_control("oversized-file");
    let bundle = synthetic_bundle();
    install_known_bundle(&control, &bundle).unwrap();
    let installed = control.join("instructions/v2").join(&bundle.hash);
    let path = installed.join("MAGNESIUM.md");
    fs::write(&path, vec![b'x'; bundle.files["MAGNESIUM.md"].len() + 1]).unwrap();
    assert!(matches!(
        validate_installed_dir(&installed, &bundle),
        Err(BundleError::FileMismatch)
    ));
    fs::remove_dir_all(control).unwrap();
}

#[test]
fn file_growth_between_metadata_check_and_open_stays_bounded() {
    let path =
        std::env::temp_dir().join(format!("magnesium-instruction-race-{}", std::process::id()));
    fs::write(&path, b"small").unwrap();
    let result = super::read_installed_file_after_metadata(&path, 5, || {
        fs::write(&path, vec![b'x'; 1024 * 1024]).unwrap();
    });
    assert!(matches!(result, Err(BundleError::FileMismatch)));
    fs::remove_file(path).unwrap();
}
