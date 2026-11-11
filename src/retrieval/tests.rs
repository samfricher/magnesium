use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    DiscoveryDisposition, ExclusionReason, SensitivityClass, SourceKind, discover_root,
    wildcard_match,
};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

#[test]
fn discovery_is_workspace_bounded_and_applies_authority_boundaries() {
    let fixture = Fixture::new("bounded");
    fixture.write("source.rs", b"fn source() {}\n");
    fixture.write("ignored-by-git.md", b"still authoritative\n");
    fixture.write("textbook.pdf", b"ignored by Magnesium\n");
    fixture.write(".gitignore", b"ignored-by-git.md\n");
    fixture.write(
        &format!("{}/magnesiumignore", crate::workspace::CONTROL_DIR),
        b"textbook.pdf\n",
    );
    let canonical_sentinel = b"canonical learner state must not change";
    fixture.write(
        &format!("{}/state.sqlite", crate::workspace::CONTROL_DIR),
        canonical_sentinel,
    );
    fixture.write("target/generated.rs", b"generated\n");
    fixture.write(".aws/credentials", b"secret\n");
    fixture.write("nested/.git/HEAD", b"ref: refs/heads/main\n");
    fixture.write("nested/source.rs", b"nested repository source\n");
    fixture.write("Learning Notes/topic.md", b"the learner's own note\n");
    fixture.write(".env", b"TOKEN=secret\n");
    #[cfg(unix)]
    let outside = fixture.root.with_extension("outside.txt");
    #[cfg(unix)]
    fs::write(&outside, b"outside\n").unwrap();
    #[cfg(unix)]
    fixture.symlink(&outside, "escape.txt");

    let report = discover_root(&fixture.root).expect("discover sources");

    assert_source(&report.entries, "source.rs", SourceKind::Text);
    assert_source(&report.entries, "ignored-by-git.md", SourceKind::Text);
    assert_excluded(
        &report.entries,
        "textbook.pdf",
        ExclusionReason::Magnesiumignore,
    );
    assert_excluded(&report.entries, "target", ExclusionReason::BuiltIn);
    assert_excluded(
        &report.entries,
        crate::workspace::CONTROL_DIR,
        ExclusionReason::BuiltIn,
    );
    assert_excluded(
        &report.entries,
        ".aws",
        ExclusionReason::Sensitive(SensitivityClass::Credential),
    );
    assert_excluded(&report.entries, "nested", ExclusionReason::NestedRepository);
    assert_source(&report.entries, "Learning Notes/topic.md", SourceKind::Text);
    assert_excluded(
        &report.entries,
        ".env",
        ExclusionReason::Sensitive(SensitivityClass::Environment),
    );
    #[cfg(unix)]
    assert_excluded(&report.entries, "escape.txt", ExclusionReason::Symlink);
    #[cfg(unix)]
    assert!(
        !report
            .entries
            .iter()
            .any(|entry| entry.relative_path == outside)
    );
    assert_eq!(
        fs::read(
            fixture
                .root
                .join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite")
        )
        .unwrap(),
        canonical_sentinel
    );
    #[cfg(unix)]
    fs::remove_file(outside).unwrap();
}

#[test]
fn magnesiumignore_negation_cannot_promote_protected_content() {
    let fixture = Fixture::new("override");
    fixture.write(
        &format!("{}/magnesiumignore", crate::workspace::CONTROL_DIR),
        b"*.md\n!keep.md\n!.env\n!target/generated.rs\n",
    );
    fixture.write("drop.md", b"drop\n");
    fixture.write("keep.md", b"keep\n");
    fixture.write(".env", b"SECRET=value\n");
    fixture.write("target/generated.rs", b"generated\n");

    let report = discover_root(&fixture.root).expect("discover sources");

    assert_excluded(&report.entries, "drop.md", ExclusionReason::Magnesiumignore);
    assert_source(&report.entries, "keep.md", SourceKind::Text);
    assert_excluded(
        &report.entries,
        ".env",
        ExclusionReason::Sensitive(SensitivityClass::Environment),
    );
    assert_excluded(&report.entries, "target", ExclusionReason::BuiltIn);
}

#[test]
fn sensitive_unsupported_and_huge_files_record_metadata_without_content() {
    let fixture = Fixture::new("metadata-only");
    fixture.write("api-credentials.json", b"do not read\n");
    fixture.write("api_key.txt", b"do not read\n");
    fixture.write("api-key.txt", b"do not read\n");
    fixture.write("archive.bin", b"unsupported\n");
    fixture.create_sparse("huge.txt", super::MAX_SOURCE_BYTES as u64 + 1);
    fixture.create_sparse("textbook.pdf", super::MAX_SOURCE_BYTES as u64 + 1024 * 1024);
    fixture.create_sparse("oversized.pdf", super::MAX_PDF_SOURCE_BYTES as u64 + 1);

    let report = discover_root(&fixture.root).expect("discover sources");

    assert_excluded(
        &report.entries,
        "api-credentials.json",
        ExclusionReason::Sensitive(SensitivityClass::Credential),
    );
    for path in ["api_key.txt", "api-key.txt"] {
        assert_excluded(
            &report.entries,
            path,
            ExclusionReason::Sensitive(SensitivityClass::Credential),
        );
    }
    assert_excluded(&report.entries, "archive.bin", ExclusionReason::Unsupported);
    assert_excluded(
        &report.entries,
        "huge.txt",
        ExclusionReason::TooLarge {
            size: super::MAX_SOURCE_BYTES as u64 + 1,
            maximum: super::MAX_SOURCE_BYTES as u64,
        },
    );
    assert_eq!(
        find(&report.entries, "textbook.pdf").disposition,
        DiscoveryDisposition::Source(SourceKind::Pdf)
    );
    assert_excluded(
        &report.entries,
        "oversized.pdf",
        ExclusionReason::TooLarge {
            size: super::MAX_PDF_SOURCE_BYTES as u64 + 1,
            maximum: super::MAX_PDF_SOURCE_BYTES as u64,
        },
    );
    let huge = find(&report.entries, "huge.txt");
    assert_eq!(huge.size, Some(super::MAX_SOURCE_BYTES as u64 + 1));
    let textbook = find(&report.entries, "textbook.pdf");
    assert_eq!(
        textbook.size,
        Some(super::MAX_SOURCE_BYTES as u64 + 1024 * 1024)
    );
}

#[test]
fn traversal_and_symlinked_control_configuration_fail_closed() {
    let traversal = Fixture::new("traversal");
    traversal.write(
        &format!("{}/magnesiumignore", crate::workspace::CONTROL_DIR),
        b"../outside.md\n",
    );
    assert!(
        discover_root(&traversal.root)
            .unwrap_err()
            .to_string()
            .contains("`..`")
    );

    #[cfg(unix)]
    {
        let linked = Fixture::new("linked-control");
        let outside = linked.root.with_extension("outside-control");
        fs::create_dir(&outside).unwrap();
        linked.symlink(&outside, crate::workspace::CONTROL_DIR);
        assert!(
            discover_root(&linked.root)
                .unwrap_err()
                .to_string()
                .contains("symbolic link")
        );
        assert!(fs::read_dir(&outside).unwrap().next().is_none());
        fs::remove_dir(outside).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_recorded_and_discovery_continues() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Fixture::new("permission");
    fixture.write("readable.rs", b"fn readable() {}\n");
    fixture.write("private/secret.rs", b"fn hidden() {}\n");
    let private = fixture.root.join("private");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o000)).unwrap();

    let report = discover_root(&fixture.root).expect("discover with permission failure");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();

    assert_source(&report.entries, "readable.rs", SourceKind::Text);
    assert_excluded(
        &report.entries,
        "private",
        ExclusionReason::Unreadable(io::ErrorKind::PermissionDenied),
    );
}

#[test]
fn wildcard_matching_is_bounded_to_declared_pattern_features() {
    assert!(wildcard_match("*.rs", "source.rs"));
    assert!(wildcard_match("file-??.md", "file-01.md"));
    assert!(!wildcard_match("*.rs", "source.py"));
    assert!(wildcard_match("*.md", "*notes.md"));
    assert!(wildcard_match("a*", "a*b"));
    assert!(wildcard_match("*", "*"));
    assert!(!wildcard_match("*.md", "*notes.txt"));
}

#[test]
fn sensitive_directory_names_match_in_any_ascii_case() {
    let fixture = Fixture::new("sensitive-case");
    fixture.write(".AWS/notes.md", b"credential notes\n");
    fixture.write(".Ssh/notes.md", b"key notes\n");
    fixture.write(".Docker/config.json", b"{}\n");

    let report = discover_root(&fixture.root).expect("discover sources");

    assert_excluded(
        &report.entries,
        ".AWS",
        ExclusionReason::Sensitive(SensitivityClass::Credential),
    );
    assert_excluded(
        &report.entries,
        ".Ssh",
        ExclusionReason::Sensitive(SensitivityClass::PrivateKey),
    );
    assert_excluded(
        &report.entries,
        ".Docker",
        ExclusionReason::Sensitive(SensitivityClass::Credential),
    );
}

#[test]
fn ignore_rules_match_file_names_that_contain_a_star() {
    let fixture = Fixture::new("star-name");
    fixture.write(
        &format!("{}/magnesiumignore", crate::workspace::CONTROL_DIR),
        b"*.md\n",
    );
    fixture.write("*notes.md", b"private notes\n");
    fixture.write("kept.rs", b"fn kept() {}\n");

    let report = discover_root(&fixture.root).expect("discover sources");

    assert_excluded(
        &report.entries,
        "*notes.md",
        ExclusionReason::Magnesiumignore,
    );
    assert_source(&report.entries, "kept.rs", SourceKind::Text);
}

fn assert_source(entries: &[super::DiscoveryEntry], path: &str, kind: SourceKind) {
    assert_eq!(
        find(entries, path).disposition,
        DiscoveryDisposition::Source(kind)
    );
}

fn assert_excluded(entries: &[super::DiscoveryEntry], path: &str, reason: ExclusionReason) {
    assert_eq!(
        find(entries, path).disposition,
        DiscoveryDisposition::Excluded(reason)
    );
}

fn find<'a>(entries: &'a [super::DiscoveryEntry], path: &str) -> &'a super::DiscoveryEntry {
    entries
        .iter()
        .find(|entry| entry.relative_path == Path::new(path))
        .unwrap_or_else(|| panic!("missing discovery entry {path:?}: {entries:#?}"))
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magnesium-discovery-{}-{name}-{nanos}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self { root }
    }

    fn write(&self, relative: &str, contents: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn create_sparse(&self, relative: &str, size: u64) {
        let path = self.root.join(relative);
        let file = fs::File::create(path).unwrap();
        file.set_len(size).unwrap();
    }

    #[cfg(unix)]
    fn symlink(&self, target: &Path, relative: &str) {
        std::os::unix::fs::symlink(target, self.root.join(relative)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Ok(metadata) = fs::metadata(self.root.join("private")) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(
                self.root.join("private"),
                fs::Permissions::from_mode(metadata.permissions().mode() | 0o700),
            );
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}
