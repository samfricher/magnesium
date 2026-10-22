//! Workspace discovery returns untrusted source candidates only. A discovery result
//! carries no authority to mutate learner state, invoke tools, or change policy.

use std::error::Error;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

mod boundary;
pub(crate) mod catch_up;
mod chunking;
mod coordinator;
#[cfg(test)]
mod evaluation;
mod manifest;
mod search;
pub(crate) use boundary::{RetrievalRequest, retrieve_workspace_sources};
pub(crate) use coordinator::CoordinatorBudget;
pub(crate) use coordinator::OperationCancellation;
pub(crate) use manifest::ManifestError;
pub(crate) use manifest::convert::worker_status as conversion_worker_status;
pub(crate) use manifest::extract::sensitive_content;
pub(crate) use manifest::run_pdf_extractor;
pub(crate) use manifest::schema::{
    INDEX_DIAGNOSTIC_BUDGET, INDEX_INITIALIZATION_BUDGET, initialize_index,
    validate_index_integrity,
};
pub(crate) use manifest::source::{OpenSource, open_workspace_source};
pub(crate) use search::{MAX_RESULTS as MAX_SEARCH_RESULTS, SourceSearchRequest};

#[cfg(test)]
pub(crate) use manifest::status::SpanFact;
pub(crate) use manifest::status::{
    CandidateFacts, FactsInterest, Interval, ManifestRow, PreparationDisposition,
    SOURCES_EXCERPT_CHAR_CAP,
};
#[cfg(test)]
pub(crate) mod manifest_test_facts {
    pub(crate) use super::manifest::status::{
        load_current_candidates, read_manifest_row, read_manifest_row_from,
    };
}

pub(crate) fn prepare_explicit_source(
    root: &Path,
    relative: &Path,
    interest: FactsInterest,
    cancellation: &OperationCancellation,
) -> Result<manifest::status::SelectedSourceSnapshot, ManifestError> {
    cancellation.check()?;
    #[cfg(test)]
    crate::request::test_checkpoint("explicit_source_before_prepare");
    cancellation.check()?;
    let index = crate::workspace::source_index_path(root);
    let (preparation, published) =
        match coordinator::prepare_explicit_source_query(root, &index, relative, cancellation) {
            Ok(prepared) => (
                match prepared.reconciliation {
                    coordinator::ReconciliationOutcome::Reconciled => {
                        PreparationDisposition::Complete
                    }
                    coordinator::ReconciliationOutcome::DeferredBudget => {
                        PreparationDisposition::DeferredBudget
                    }
                    coordinator::ReconciliationOutcome::DeferredConcurrentUpdate => {
                        PreparationDisposition::DeferredConcurrentUpdate
                    }
                },
                None,
            ),
            Err(ManifestError::PdfContentPublished(failure)) => {
                (PreparationDisposition::PdfContentFailure, Some(failure))
            }
            Err(ManifestError::SourceFailurePublished(error)) => return Err(*error),
            Err(error) => return Err(error),
        };
    cancellation.check()?;
    manifest::status::read_selected_facts(
        &index,
        relative,
        interest,
        preparation,
        published.as_ref(),
        cancellation,
    )
}

const MAX_MAGNESIUMIGNORE_BYTES: usize = 1024 * 1024;
const MAX_MAGNESIUMIGNORE_RULES: usize = 4096;
const MAX_PATTERN_BYTES: usize = 4096;
pub(crate) const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_PDF_SOURCE_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const EXPORT_BOUNDARY_MARKER: &str = ".magnesium-export";

const EXCLUDED_DIRECTORIES: &[&str] = &[
    ".git",
    crate::workspace::CONTROL_DIR,
    "node_modules",
    "vendor",
    "target",
    "build",
    "dist",
    "out",
    ".cache",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".next",
    ".nuxt",
    ".gradle",
    "coverage",
];

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "adoc", "bash", "c", "cc", "cpp", "css", "csv", "fish", "go", "h", "hpp", "htm", "html",
    "java", "js", "json", "jsx", "kt", "kts", "less", "md", "mdx", "mjs", "mts", "php", "ps1",
    "py", "rb", "rs", "rst", "scss", "sh", "sql", "swift", "toml", "ts", "tsx", "txt", "lock",
    "xml", "yaml", "yml", "zsh", "cjs", "cts",
];

const SUPPORTED_EXTENSIONLESS_NAMES: &[&str] =
    &["Dockerfile", "Gemfile", "LICENSE", "Makefile", "README"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SourceKind {
    Text,
    Pdf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SensitivityClass {
    Environment,
    Credential,
    PrivateKey,
}

impl SensitivityClass {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment",
            Self::Credential => "credential",
            Self::PrivateKey => "private_key",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ExclusionReason {
    BuiltIn,
    Magnesiumignore,
    NestedRepository,
    PrivateExport,
    Sensitive(SensitivityClass),
    Symlink,
    TooLarge { size: u64, maximum: u64 },
    Unsupported,
    Unreadable(io::ErrorKind),
    EscapesWorkspace,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum DiscoveryDisposition {
    Source(SourceKind),
    Excluded(ExclusionReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryEntry {
    pub(crate) relative_path: PathBuf,
    pub(crate) size: Option<u64>,
    pub(crate) disposition: DiscoveryDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DiscoveryReport {
    pub(crate) workspace_root: PathBuf,
    pub(crate) entries: Vec<DiscoveryEntry>,
}

#[derive(Debug)]
pub(crate) enum DiscoveryError {
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    InvalidMagnesiumignore {
        path: PathBuf,
        line: Option<usize>,
        message: String,
    },
    InvalidWorkspace {
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "cannot {action} {}: {source}", path.display()),
            Self::InvalidMagnesiumignore {
                path,
                line: Some(line),
                message,
            } => write!(
                formatter,
                "invalid magnesiumignore {} at line {line}: {message}",
                path.display()
            ),
            Self::InvalidMagnesiumignore {
                path,
                line: None,
                message,
            } => write!(
                formatter,
                "invalid magnesiumignore {}: {message}",
                path.display()
            ),
            Self::InvalidWorkspace { path, message } => {
                write!(
                    formatter,
                    "invalid discovery workspace {}: {message}",
                    path.display()
                )
            }
        }
    }
}

impl Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::InvalidMagnesiumignore { .. } | Self::InvalidWorkspace { .. } => None,
        }
    }
}

#[cfg(test)]
pub(crate) fn discover_root(root: &Path) -> Result<DiscoveryReport, DiscoveryError> {
    let workspace_root = fs::canonicalize(root).map_err(|source| DiscoveryError::Io {
        action: "resolve discovery workspace",
        path: root.to_path_buf(),
        source,
    })?;
    if !workspace_root.is_dir() {
        return Err(DiscoveryError::InvalidWorkspace {
            path: workspace_root,
            message: "workspace root is not a directory".to_owned(),
        });
    }
    reject_symlinked_control(&workspace_root)?;
    if export_boundary_exists(&workspace_root).map_err(|source| DiscoveryError::Io {
        action: "inspect workspace export boundary",
        path: workspace_root.clone(),
        source,
    })? {
        return Ok(DiscoveryReport {
            workspace_root,
            entries: vec![excluded(
                PathBuf::from("."),
                None,
                ExclusionReason::PrivateExport,
            )],
        });
    }
    let rules = read_magnesiumignore(&workspace_root)?;
    let mut report = DiscoveryReport {
        workspace_root: workspace_root.clone(),
        entries: Vec::new(),
    };
    walk_directory(
        &workspace_root,
        Path::new(""),
        &workspace_root,
        &rules,
        &mut report.entries,
    );
    Ok(report)
}

fn reject_symlinked_control(root: &Path) -> Result<(), DiscoveryError> {
    let control = root.join(crate::workspace::CONTROL_DIR);
    match fs::symlink_metadata(&control) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(DiscoveryError::InvalidWorkspace {
                path: control,
                message: format!("{} is a symbolic link", crate::workspace::CONTROL_DIR),
            })
        }
        Ok(metadata) if !metadata.is_dir() => Err(DiscoveryError::InvalidWorkspace {
            path: control,
            message: format!("{} is not a directory", crate::workspace::CONTROL_DIR),
        }),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DiscoveryError::Io {
            action: "inspect workspace control directory",
            path: control,
            source,
        }),
    }
}

/// Current discovery policy for one workspace-relative file: built-in and
/// sensitive directory/file classification, nested-repository boundaries,
/// the control directory's `magnesiumignore` rules, and the discovery symlink
/// policy, checked for every ancestor component as well as the file itself.
/// `Ok(true)` means the file is excluded (revoked). `Err` means the policy
/// itself is unreadable or invalid, which is service unavailability, not
/// exclusion. Discovery, the coordinator, and live source verification share
/// this owner.
pub(crate) fn source_relative_path_is_excluded(
    workspace_root: &Path,
    relative: &Path,
) -> Result<bool, DiscoveryError> {
    let workspace_root = fs::canonicalize(workspace_root).map_err(|source| DiscoveryError::Io {
        action: "resolve discovery workspace",
        path: workspace_root.to_path_buf(),
        source,
    })?;
    reject_symlinked_control(&workspace_root)?;
    // Initializing a copied export must not turn its root into subject
    // material. Unlike the control directory and .git, the export marker
    // excludes the root too.
    if export_boundary_exists(&workspace_root).map_err(|source| DiscoveryError::Io {
        action: "inspect workspace export boundary",
        path: workspace_root.clone(),
        source,
    })? {
        return Ok(true);
    }
    let rules = read_magnesiumignore(&workspace_root)?;
    let components: Vec<Component> = relative.components().collect();
    let mut prefix = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            // Absolute, parent, or root components never name a workspace file.
            return Ok(true);
        };
        prefix.push(name);
        // Discovery never follows a symlink, so a symlinked component revokes
        // the path even when its canonical target stays inside the workspace;
        // this also closes the alias to a sensitive or built-in directory.
        let component_path = workspace_root.join(&prefix);
        match fs::symlink_metadata(&component_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Ok(true),
            Ok(metadata) if index + 1 < components.len() && !metadata.is_dir() => {
                return Ok(true);
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(DiscoveryError::Io {
                    action: "inspect source component",
                    path: component_path,
                    source,
                });
            }
        }
        if index + 1 < components.len() {
            if classify_sensitive_directory(name).is_some()
                || is_excluded_directory(name)
                || rules_exclude(&rules, &prefix, true)
            {
                return Ok(true);
            }
            match excluded_source_boundary(&component_path) {
                Ok(Some(_)) => return Ok(true),
                Ok(None) => {}
                Err(source) => {
                    return Err(DiscoveryError::Io {
                        action: "inspect excluded source boundary",
                        path: component_path,
                        source,
                    });
                }
            }
        } else if classify_sensitivity(&prefix).is_some() || rules_exclude(&rules, &prefix, false) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
fn walk_directory(
    root: &Path,
    relative_directory: &Path,
    directory: &Path,
    rules: &[IgnoreRule],
    entries: &mut Vec<DiscoveryEntry>,
) {
    let iterator = match fs::read_dir(directory) {
        Ok(iterator) => iterator,
        Err(error) => {
            entries.push(excluded(
                display_relative(relative_directory),
                None,
                ExclusionReason::Unreadable(error.kind()),
            ));
            return;
        }
    };
    let mut children = Vec::new();
    for child in iterator {
        match child {
            Ok(child) => children.push(child),
            Err(error) => entries.push(excluded(
                display_relative(relative_directory),
                None,
                ExclusionReason::Unreadable(error.kind()),
            )),
        }
    }
    children.sort_by_key(fs::DirEntry::file_name);

    for child in children {
        let path = child.path();
        let relative = relative_directory.join(child.file_name());
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) => {
                entries.push(excluded(
                    relative,
                    None,
                    ExclusionReason::Unreadable(error.kind()),
                ));
                continue;
            }
        };
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            entries.push(excluded(relative, None, ExclusionReason::Symlink));
        } else if file_type.is_dir() {
            walk_child_directory(root, &relative, &path, rules, entries);
        } else if file_type.is_file() {
            classify_file(&relative, metadata.len(), rules, entries);
        } else {
            entries.push(excluded(
                relative,
                Some(metadata.len()),
                ExclusionReason::Unsupported,
            ));
        }
    }
}

#[cfg(test)]
fn walk_child_directory(
    root: &Path,
    relative: &Path,
    path: &Path,
    rules: &[IgnoreRule],
    entries: &mut Vec<DiscoveryEntry>,
) {
    let Some(name) = relative.file_name() else {
        entries.push(excluded(
            relative.to_path_buf(),
            None,
            ExclusionReason::Unsupported,
        ));
        return;
    };
    if let Some(sensitivity) = classify_sensitive_directory(name) {
        entries.push(excluded(
            relative.to_path_buf(),
            None,
            ExclusionReason::Sensitive(sensitivity),
        ));
        return;
    }
    if is_excluded_directory(name) {
        entries.push(excluded(
            relative.to_path_buf(),
            None,
            ExclusionReason::BuiltIn,
        ));
        return;
    }
    match excluded_source_boundary(path) {
        Ok(Some(reason)) => {
            entries.push(excluded(relative.to_path_buf(), None, reason));
            return;
        }
        Ok(None) => {}
        Err(error) => {
            entries.push(excluded(
                relative.to_path_buf(),
                None,
                ExclusionReason::Unreadable(error.kind()),
            ));
            return;
        }
    }
    if rules_exclude(rules, relative, true) {
        entries.push(excluded(
            relative.to_path_buf(),
            None,
            ExclusionReason::Magnesiumignore,
        ));
        return;
    }
    match fs::canonicalize(path) {
        Ok(canonical) if canonical.starts_with(root) => {
            walk_directory(root, relative, path, rules, entries);
        }
        Ok(_) => entries.push(excluded(
            relative.to_path_buf(),
            None,
            ExclusionReason::EscapesWorkspace,
        )),
        Err(error) => entries.push(excluded(
            relative.to_path_buf(),
            None,
            ExclusionReason::Unreadable(error.kind()),
        )),
    }
}

fn classify_file(
    relative: &Path,
    size: u64,
    rules: &[IgnoreRule],
    entries: &mut Vec<DiscoveryEntry>,
) {
    if let Some(sensitivity) = classify_sensitivity(relative) {
        entries.push(excluded(
            relative.to_path_buf(),
            Some(size),
            ExclusionReason::Sensitive(sensitivity),
        ));
        return;
    }
    if rules_exclude(rules, relative, false) {
        entries.push(excluded(
            relative.to_path_buf(),
            Some(size),
            ExclusionReason::Magnesiumignore,
        ));
        return;
    }
    let kind = match source_kind(relative) {
        Some(kind) => kind,
        None => {
            let reason = if size > MAX_SOURCE_BYTES as u64 {
                ExclusionReason::TooLarge {
                    size,
                    maximum: MAX_SOURCE_BYTES as u64,
                }
            } else {
                ExclusionReason::Unsupported
            };
            entries.push(excluded(relative.to_path_buf(), Some(size), reason));
            return;
        }
    };
    let maximum = match kind {
        SourceKind::Text => MAX_SOURCE_BYTES as u64,
        SourceKind::Pdf => MAX_PDF_SOURCE_BYTES as u64,
    };
    if size > maximum {
        entries.push(excluded(
            relative.to_path_buf(),
            Some(size),
            ExclusionReason::TooLarge { size, maximum },
        ));
        return;
    }
    entries.push(DiscoveryEntry {
        relative_path: relative.to_path_buf(),
        size: Some(size),
        disposition: DiscoveryDisposition::Source(kind),
    });
}

fn excluded(relative_path: PathBuf, size: Option<u64>, reason: ExclusionReason) -> DiscoveryEntry {
    DiscoveryEntry {
        relative_path,
        size,
        disposition: DiscoveryDisposition::Excluded(reason),
    }
}

fn display_relative(relative: &Path) -> PathBuf {
    if relative.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        relative.to_path_buf()
    }
}

fn excluded_source_boundary(path: &Path) -> io::Result<Option<ExclusionReason>> {
    // Presence is sufficient, including a symlink or malformed marker. This
    // is a fail-closed source exclusion, never workspace/store authority.
    if export_boundary_exists(path)? {
        return Ok(Some(ExclusionReason::PrivateExport));
    }
    for (marker, reason) in [
        (".git", ExclusionReason::NestedRepository),
        (
            crate::workspace::CONTROL_DIR,
            ExclusionReason::NestedRepository,
        ),
    ] {
        match fs::symlink_metadata(path.join(marker)) {
            Ok(_) => return Ok(Some(reason)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(None)
}

fn export_boundary_exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path.join(EXPORT_BOUNDARY_MARKER)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn is_excluded_directory(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        EXCLUDED_DIRECTORIES
            .iter()
            .any(|excluded| name.eq_ignore_ascii_case(excluded))
    })
}

fn classify_sensitivity(path: &Path) -> Option<SensitivityClass> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();
    if name == ".env" || name.starts_with(".env.") {
        return Some(SensitivityClass::Environment);
    }
    if matches!(
        name.as_str(),
        ".netrc" | ".npmrc" | ".pypirc" | ".htpasswd" | "kubeconfig" | "service-account.json"
    ) || (name == "config.json"
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|parent| parent == OsStr::new(".docker")))
    {
        return Some(SensitivityClass::Credential);
    }
    let extension = path
        .extension()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    if extension
        .as_deref()
        .is_some_and(|extension| matches!(extension, "key" | "pem" | "p12" | "pfx"))
        || matches!(
            name.as_str(),
            "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519"
        )
    {
        return Some(SensitivityClass::PrivateKey);
    }
    let stem = path
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or(&name)
        .to_ascii_lowercase();
    let tokens = stem.split(|character: char| !character.is_ascii_alphanumeric());
    if tokens.into_iter().any(|token| {
        matches!(
            token,
            "credential"
                | "credentials"
                | "key"
                | "keys"
                | "secret"
                | "secrets"
                | "token"
                | "tokens"
        )
    }) {
        Some(SensitivityClass::Credential)
    } else {
        None
    }
}

fn classify_sensitive_directory(name: &OsStr) -> Option<SensitivityClass> {
    match name.to_str()? {
        ".ssh" | ".gnupg" => Some(SensitivityClass::PrivateKey),
        ".aws" | ".azure" | ".docker" | ".kube" => Some(SensitivityClass::Credential),
        _ => None,
    }
}

/// Discovery classification for one workspace-relative path. Live source
/// verification shares this owner so an anchor cannot authorize a path whose
/// extension discovery would mark unsupported or of another source kind.
pub(crate) fn source_kind(path: &Path) -> Option<SourceKind> {
    let extension = match path.extension().and_then(OsStr::to_str) {
        Some(extension) => extension,
        None => {
            let name = path.file_name()?.to_str()?;
            return SUPPORTED_EXTENSIONLESS_NAMES
                .iter()
                .any(|supported| name.eq_ignore_ascii_case(supported))
                .then_some(SourceKind::Text);
        }
    };
    if extension.eq_ignore_ascii_case("pdf") {
        return Some(SourceKind::Pdf);
    }
    if SUPPORTED_EXTENSIONS
        .iter()
        .any(|supported| extension.eq_ignore_ascii_case(supported))
    {
        return Some(SourceKind::Text);
    }
    None
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IgnoreRule {
    negated: bool,
    directory_only: bool,
    anchored: bool,
    segments: Vec<String>,
}

fn read_magnesiumignore(root: &Path) -> Result<Vec<IgnoreRule>, DiscoveryError> {
    let path = root
        .join(crate::workspace::CONTROL_DIR)
        .join("magnesiumignore");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(DiscoveryError::Io {
                action: "inspect magnesiumignore",
                path,
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(DiscoveryError::InvalidMagnesiumignore {
            path,
            line: None,
            message: "file must be a regular file, not a symbolic link".to_owned(),
        });
    }
    if metadata.len() > MAX_MAGNESIUMIGNORE_BYTES as u64 {
        return Err(DiscoveryError::InvalidMagnesiumignore {
            path,
            line: None,
            message: format!("file exceeds the {MAX_MAGNESIUMIGNORE_BYTES}-byte limit"),
        });
    }
    let mut contents = String::new();
    fs::File::open(&path)
        .and_then(|file| {
            file.take((MAX_MAGNESIUMIGNORE_BYTES + 1) as u64)
                .read_to_string(&mut contents)
        })
        .map_err(|source| DiscoveryError::Io {
            action: "read magnesiumignore",
            path: path.clone(),
            source,
        })?;
    if contents.len() > MAX_MAGNESIUMIGNORE_BYTES {
        return Err(DiscoveryError::InvalidMagnesiumignore {
            path,
            line: None,
            message: format!("file exceeds the {MAX_MAGNESIUMIGNORE_BYTES}-byte limit"),
        });
    }

    let mut rules = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        let rule =
            parse_ignore_rule(line).map_err(|message| DiscoveryError::InvalidMagnesiumignore {
                path: path.clone(),
                line: Some(index + 1),
                message,
            })?;
        if let Some(rule) = rule {
            if rules.len() == MAX_MAGNESIUMIGNORE_RULES {
                return Err(DiscoveryError::InvalidMagnesiumignore {
                    path,
                    line: Some(index + 1),
                    message: format!("file exceeds the {MAX_MAGNESIUMIGNORE_RULES}-rule limit"),
                });
            }
            rules.push(rule);
        }
    }
    Ok(rules)
}

fn parse_ignore_rule(line: &str) -> Result<Option<IgnoreRule>, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    if line.len() > MAX_PATTERN_BYTES {
        return Err(format!(
            "pattern exceeds the {MAX_PATTERN_BYTES}-byte limit"
        ));
    }
    let (negated, pattern) = line
        .strip_prefix('!')
        .map_or((false, line), |pattern| (true, pattern));
    let anchored = pattern.starts_with('/');
    let pattern = pattern.trim_start_matches('/');
    let directory_only = pattern.ends_with('/');
    let pattern = pattern.trim_end_matches('/');
    if pattern.is_empty() {
        return Err("pattern must name a workspace-relative path".to_owned());
    }
    let segments: Vec<String> = pattern.split('/').map(ToOwned::to_owned).collect();
    if segments
        .iter()
        .any(|segment| segment.is_empty() || matches!(segment.as_str(), "." | ".."))
    {
        return Err("pattern cannot contain empty, `.` or `..` path segments".to_owned());
    }
    Ok(Some(IgnoreRule {
        negated,
        directory_only,
        anchored,
        segments,
    }))
}

fn rules_exclude(rules: &[IgnoreRule], relative: &Path, is_directory: bool) -> bool {
    let Some(path) = path_segments(relative) else {
        return false;
    };
    rules.iter().fold(false, |excluded, rule| {
        if rule.matches(&path, is_directory) {
            !rule.negated
        } else {
            excluded
        }
    })
}

impl IgnoreRule {
    fn matches(&self, path: &[String], is_directory: bool) -> bool {
        if self.directory_only && !is_directory {
            return false;
        }
        if self.anchored || self.segments.len() > 1 {
            match_segments(&self.segments, path)
        } else {
            path.iter()
                .any(|segment| wildcard_match(&self.segments[0], segment))
        }
    }
}

fn path_segments(path: &Path) -> Option<Vec<String>> {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(ToOwned::to_owned),
            _ => None,
        })
        .collect()
}

fn match_segments(pattern: &[String], path: &[String]) -> bool {
    let (mut pattern_index, mut path_index) = (0, 0);
    let (mut globstar, mut retry_path) = (None, 0);
    while path_index < path.len() {
        if pattern_index < pattern.len()
            && pattern[pattern_index] != "**"
            && wildcard_match(&pattern[pattern_index], &path[path_index])
        {
            pattern_index += 1;
            path_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == "**" {
            globstar = Some(pattern_index);
            retry_path = path_index;
            pattern_index += 1;
        } else if let Some(star) = globstar {
            retry_path += 1;
            path_index = retry_path;
            pattern_index = star + 1;
        } else {
            return false;
        }
    }
    pattern[pattern_index..]
        .iter()
        .all(|segment| segment == "**")
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let (mut pattern_index, mut value_index) = (0, 0);
    let (mut star, mut retry_value) = (None, 0);
    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star = Some(pattern_index);
            retry_value = value_index;
            pattern_index += 1;
        } else if let Some(star_index) = star {
            retry_value += 1;
            value_index = retry_value;
            pattern_index = star_index + 1;
        } else {
            return false;
        }
    }
    pattern[pattern_index..].iter().all(|value| *value == '*')
}

#[cfg(test)]
mod tests {
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
}

#[cfg(test)]
mod learner_workspace_boundary_tests {
    use super::*;
    #[test]
    fn nested_initialized_workspace_is_not_parent_source_material() {
        let root =
            std::env::temp_dir().join(crate::learner::store::new_id("source-scope").unwrap());
        fs::create_dir_all(root.join("course").join(crate::workspace::CONTROL_DIR)).unwrap();
        fs::write(root.join("course/private.md"), "another learner workspace").unwrap();
        fs::write(root.join("shared.md"), "parent material").unwrap();
        assert!(source_relative_path_is_excluded(&root, Path::new("course/private.md")).unwrap());
        assert!(!source_relative_path_is_excluded(&root, Path::new("shared.md")).unwrap());
        assert_eq!(
            excluded_source_boundary(&root.join("course")).unwrap(),
            Some(ExclusionReason::NestedRepository)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
