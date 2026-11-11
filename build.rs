//! Packs the tutor's instruction files into the program.
//!
//! `src/workspace/learning/manifest.json` lists the files and what each is
//! for. The build adds each file's SHA-256 to that list, so an edit to an
//! instruction file needs no other change. It writes two files to `OUT_DIR`:
//! the list and the files as one pack, and the list's metadata as Rust.
//! `src/workspace/instruction_bundle.rs` reads both and checks the pack
//! again when the program runs.

use std::{collections::BTreeSet, env, fmt::Write as _, fs, path::Path};

use sha2::{Digest, Sha256};

const BUNDLE_DIR: &str = "src/workspace/learning";
/// The line of a manifest entry that its hash is written after.
const HASH_GOES_AFTER: &str = "\"required_for_activation\": true";

/// Read a file of the bundle. No part of its path may be a symlink.
fn read_bundle_file(relative: &str) -> Vec<u8> {
    let path = Path::new(BUNDLE_DIR).join(relative);
    for ancestor in path.ancestors().take(relative.split('/').count()) {
        let kind = fs::symlink_metadata(ancestor)
            .unwrap_or_else(|error| panic!("read {}: {error}", ancestor.display()))
            .file_type();
        assert!(
            !kind.is_symlink(),
            "bundle path is a symlink: {}",
            ancestor.display()
        );
    }
    fs::read(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

/// The role, stage and host that a file's place in the bundle gives it.
fn metadata_for(path: &str) -> (&'static str, Option<&str>, Option<&str>) {
    let name_in = |folder: &str| {
        path.strip_prefix(folder)
            .and_then(|name| name.strip_suffix(".md"))
    };
    if path == "MAGNESIUM.md" {
        ("orchestrator", None, None)
    } else if path == "teaching-policy.md" {
        ("policy", None, None)
    } else if name_in("contracts/").is_some() {
        ("contract", None, None)
    } else if let Some(stage) = name_in("workflows/") {
        ("workflow", Some(stage), None)
    } else if let Some(host) = name_in("adapters/") {
        ("adapter", None, Some(host))
    } else {
        panic!("unexpected learning bundle path: {path}")
    }
}

fn main() {
    for path in [BUNDLE_DIR, "build.rs"] {
        println!("cargo:rerun-if-changed={path}");
    }
    let authored = String::from_utf8(read_bundle_file("manifest.json")).expect("UTF-8 manifest");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&authored).expect("learning manifest JSON");
    // The packed manifest is the authored text with one line added to each
    // entry. The bundle's identity is a hash that covers these bytes, so the
    // text is kept as written: a study folder then sees a new bundle only
    // when a file or the list changes, not when this script does.
    let mut packed_manifest = String::new();
    let mut rest = authored.as_str();
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    let mut metadata = String::from(
        "const GENERATED_METADATA: &[(&str, &str, Option<&str>, Option<&str>)] = &[\n",
    );
    for entry in manifest["files"]
        .as_array_mut()
        .expect("manifest files array")
    {
        let entry = entry.as_object_mut().expect("manifest file object");
        let path = entry["path"].as_str().expect("manifest path").to_owned();
        assert!(
            !path.starts_with('/')
                && !path.contains('\\')
                && path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
                && seen.insert(path.clone()),
            "unsafe or duplicate learning manifest path: {path}"
        );
        assert!(
            !entry.contains_key("sha256"),
            "the build fills in sha256; remove it from the manifest entry of {path}"
        );
        let (role, stage, host) = metadata_for(&path);
        assert_eq!(entry["role"], role, "wrong role: {path}");
        let text = |key: &str| entry.get(key).and_then(serde_json::Value::as_str);
        assert_eq!(text("stage"), stage, "wrong stage: {path}");
        assert_eq!(text("host"), host, "wrong host: {path}");
        let content = read_bundle_file(&path);
        let digest = Sha256::digest(&content)
            .iter()
            .fold(String::new(), |mut hex, byte| {
                write!(hex, "{byte:02x}").expect("write to String");
                hex
            });
        let (before, after) = rest
            .split_once(HASH_GOES_AFTER)
            .unwrap_or_else(|| panic!("manifest entry of {path} lacks {HASH_GOES_AFTER}"));
        write!(
            packed_manifest,
            "{before}{HASH_GOES_AFTER},\n      \"sha256\": \"{digest}\""
        )
        .expect("write");
        rest = after;
        entry.insert("sha256".to_owned(), digest.into());
        writeln!(metadata, "    ({path:?}, {role:?}, {stage:?}, {host:?}),").expect("write");
        files.push((path, content));
    }
    metadata.push_str("];\nconst ALLOWED_PATHS: &[&str] = &[\n");
    for (path, _) in &files {
        writeln!(metadata, "    {path:?},").expect("write");
    }
    metadata.push_str("];\n");

    packed_manifest.push_str(rest);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&packed_manifest).expect("packed manifest JSON"),
        manifest,
        "each hash must land in its own manifest entry"
    );
    let mut pack = Vec::new();
    for (name, content) in
        std::iter::once(("manifest.json".to_owned(), packed_manifest.into_bytes())).chain(files)
    {
        pack.extend_from_slice(name.as_bytes());
        pack.push(0);
        pack.extend_from_slice(&(content.len() as u64).to_be_bytes());
        pack.extend_from_slice(&content);
    }
    let out = env::var_os("OUT_DIR").expect("OUT_DIR");
    fs::write(Path::new(&out).join("learning-manifest.rs"), metadata)
        .expect("write learning manifest metadata");
    fs::write(Path::new(&out).join("learning-instructions-v2.bin"), pack)
        .expect("write learning instruction pack");
}
