//! Allowlisted, stage-routed instruction assets. The compiled active bundle is
//! served after packaged manifest and installed content hashes are validated.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::workspace::Workspace;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const BUNDLE_VERSION: u64 = 2;
const SUPPORTED_SCHEMA: u64 = 2;
/// Caps the instruction text in one load. Across every stage and host, the MCP
/// text block adds at most about 4% of JSON escapes and 477 bytes of fields,
/// so 32 KiB of text makes an MCP text block of at most about 34,600 bytes.
/// The binding host limits are higher: Claude Code saves a result over 50,000
/// characters to a file, Codex silently cuts above about 48,000 bytes on
/// bundled models, and OpenCode truncates above 51,200 bytes.
const RESPONSE_LIMIT: usize = 32 * 1024;
const MAX_APPROVED_ASSET_BYTES: usize = 1024 * 1024;
const MAX_BUNDLE_DIRECTORY_DEPTH: usize = 4;
const MAX_APPROVED_MANIFEST_BYTES: usize = 64 * 1024;
const ACTIVE_POINTER_LIMIT: usize = 4096;
const MANIFEST: &str = "manifest.json";
const PLACEHOLDER_MARKER: &[u8] = b"<!-- RESERVED_FOR_EXPERT_AUTHORING: DO_NOT_ACTIVATE -->\n";
const ASSET_PACK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/learning-instructions-v2.bin"));
include!(concat!(env!("OUT_DIR"), "/learning-manifest.rs"));

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstructionStage {
    OrientAndProbe,
    PlanAndRoute,
    TeachAndPractice,
    InterpretAndRepair,
    ReviewAndTransfer,
    ConsolidateAndResume,
    Papers,
    PapersLoop,
}

impl InstructionStage {
    fn manifest_key(self) -> &'static str {
        match self {
            Self::OrientAndProbe => "orient-and-probe",
            Self::PlanAndRoute => "plan-and-route",
            Self::TeachAndPractice => "teach-and-practice",
            Self::InterpretAndRepair => "interpret-and-repair",
            Self::ReviewAndTransfer => "review-and-transfer",
            Self::ConsolidateAndResume => "consolidate-and-resume",
            Self::Papers => "papers",
            Self::PapersLoop => "papers-loop",
        }
    }

    pub(crate) fn workflow(self) -> &'static str {
        GENERATED_METADATA
            .iter()
            .find(|(_, role, stage, _)| *role == "workflow" && *stage == Some(self.manifest_key()))
            .map(|(path, _, _, _)| *path)
            .expect("build validates the complete workflow stage set")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstructionHost {
    Codex,
    Claude,
    Opencode,
}

impl InstructionHost {
    fn manifest_key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
        }
    }

    fn asset(self) -> &'static str {
        GENERATED_METADATA
            .iter()
            .find(|(_, role, _, host)| *role == "adapter" && *host == Some(self.manifest_key()))
            .map(|(path, _, _, _)| *path)
            .expect("build validates every runtime host adapter")
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstructionOperation {
    StateAndEvidence,
    SourceUse,
    ToolRecipes,
}

/// Packaged bundle identity and authoring state. This describes bytes in the
/// runtime package only; it does not claim that a host loaded or followed them.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstructionBundleStatus {
    Placeholder,
    Active,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstructionBundleIdentity {
    pub(crate) version: u64,
    pub(crate) hash: String,
    pub(crate) status: InstructionBundleStatus,
}

/// Return the packaged bundle identity without reading workspace files or
/// claiming that a model loaded this bundle.
pub(crate) fn current_identity() -> Result<InstructionBundleIdentity, String> {
    let bundle = compiled_bundle().map_err(|error| error.to_string())?;
    let status = match bundle.status.as_str() {
        "placeholder" => InstructionBundleStatus::Placeholder,
        "active" => InstructionBundleStatus::Active,
        _ => return Err("compiled instruction bundle has invalid status".to_owned()),
    };
    Ok(InstructionBundleIdentity {
        version: bundle.version,
        hash: bundle.hash.clone(),
        status,
    })
}

/// Report the packaged identity only when this workspace has matching bytes.
pub(crate) fn installed_identity(control: &Path) -> Result<InstructionBundleIdentity, String> {
    if fs::read(control.join(crate::adapter::INSTRUCTIONS_FILENAME))
        .ok()
        .as_deref()
        != Some(crate::adapter::HARNESS_INSTRUCTIONS.as_bytes())
    {
        return Err("workspace instruction bridge does not match this runtime".into());
    }
    let bundle = compiled_bundle().map_err(|e| e.to_string())?;
    load_installed(&control.join("instructions"), &bundle).map_err(|e| e.to_string())?;
    current_identity()
}

impl InstructionOperation {
    fn asset(self) -> &'static str {
        match self {
            Self::StateAndEvidence => "contracts/state-and-evidence.md",
            Self::SourceUse => "contracts/source-use.md",
            Self::ToolRecipes => "contracts/tool-recipes.md",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    bundle_version: u64,
    status: String,
    minimum_learning_step_schema: u64,
    entrypoint: String,
    files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: String,
    role: String,
    required_for_activation: bool,
    sha256: String,
    #[serde(default)]
    stage: Option<String>,
    #[serde(default)]
    host: Option<String>,
}

#[derive(Debug)]
struct Bundle {
    version: u64,
    hash: String,
    status: String,
    // Workspace loads reuse this immutable asset map without copying every
    // compiled file into a per-workspace bundle.
    files: Arc<BTreeMap<String, Vec<u8>>>,
}

#[derive(Clone, Debug, Serialize)]
struct InstructionLoad {
    status: &'static str,
    bundle_version: u64,
    bundle_hash: String,
    recommended_stage: InstructionStage,
    files: Vec<String>,
    deferred_modules: Vec<String>,
    bytes_served: usize,
    validation_file_count: usize,
    validation_bytes: usize,
    immutable_bytes_reused: bool,
    capability_tier: String,
    text: String,
}

#[derive(Debug)]
enum BundleError {
    UnsupportedBundle,
    UnsupportedSchema,
    Placeholder,
    ManifestMismatch,
    FileMismatch,
    UnsafePath,
    Symlink,
    UserEdits,
    PreviousBundleActive,
    ActivePointer,
    RequiredSetTooLarge,
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for BundleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::UnsupportedBundle => "unsupported bundle version".to_owned(),
            Self::UnsupportedSchema => "unsupported learning-step schema".to_owned(),
            Self::Placeholder => "bundle is a reserved placeholder".to_owned(),
            Self::ManifestMismatch => {
                "bundle manifest does not match the compiled allowlist".to_owned()
            }
            Self::FileMismatch => "bundle file is missing or has an unexpected hash".to_owned(),
            Self::UnsafePath => {
                "bundle path contains traversal or is outside the allowlist".to_owned()
            }
            Self::Symlink => {
                "bundle path contains a symbolic link or unsupported filesystem object".to_owned()
            }
            Self::UserEdits => "installed bundle differs from the reviewed bytes".to_owned(),
            Self::PreviousBundleActive => "this workspace uses the teaching instructions of an earlier Magnesium version; run `magnesium init --upgrade-instructions` in it to switch to this version's instructions (learner history is kept)".to_owned(),
            Self::ActivePointer => {
                "active bundle pointer is missing or does not match the reviewed bundle; run `magnesium init --upgrade-instructions` in this workspace".to_owned()
            }
            Self::RequiredSetTooLarge => {
                format!("required instruction set exceeds the {RESPONSE_LIMIT}-byte response limit")
            }
            Self::Io(error) => format!("filesystem operation failed: {error}"),
            Self::Json(error) => format!("invalid manifest: {error}"),
        };
        formatter.write_str(&message)
    }
}

impl std::error::Error for BundleError {}

impl From<io::Error> for BundleError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for BundleError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum InstallDisposition {
    CreatedPassive,
    VerifiedPassive,
    CreatedAndActivated,
    AlreadyActive,
    UpdatedActive,
    /// The pointer moved from a verified, unedited earlier bundle.
    Upgraded {
        previous: String,
    },
}

/// Resolve the initialized workspace locally. Instruction loading does not
/// open SQLite, consult global routing, migrate storage, or install files.
pub(crate) fn resolve_authorized_control(start: &Path) -> Result<Option<PathBuf>, String> {
    let workspace = Workspace::discover(start).map_err(|error| error.to_string())?;
    let Some(workspace) = workspace else {
        return Ok(None);
    };
    let root = fs::canonicalize(workspace.root()).map_err(|error| error.to_string())?;
    let control = root.join(crate::workspace::CONTROL_DIR);
    let control_metadata = fs::symlink_metadata(&control).map_err(|error| error.to_string())?;
    if control_metadata.file_type().is_symlink() || !control_metadata.is_dir() {
        return Err("Magnesium control path is not a real directory".to_owned());
    }
    let config_path = control.join("config.toml");
    let config_metadata = fs::symlink_metadata(&config_path).map_err(|error| error.to_string())?;
    if config_metadata.file_type().is_symlink() || !config_metadata.is_file() {
        return Err("Magnesium config is not a regular file".to_owned());
    }
    workspace.id().map_err(|error| error.to_string())?;
    Ok(Some(control))
}

static COMPILED: OnceLock<Result<Arc<Bundle>, String>> = OnceLock::new();
fn compiled_bundle() -> Result<Arc<Bundle>, BundleError> {
    let result = COMPILED.get_or_init(|| {
        let files = unpack_asset_pack(ASSET_PACK).map_err(|e| e.to_string())?;
        let manifest = files.get(MANIFEST).ok_or("manifest missing from package")?;
        let bundle = validate_bundle(manifest.clone(), files.clone()).map_err(|e| e.to_string())?;
        Ok(Arc::new(bundle))
    });
    result
        .as_ref()
        .map(Arc::clone)
        .map_err(|_| BundleError::ManifestMismatch)
}

fn unpack_asset_pack(mut bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, BundleError> {
    let mut files = BTreeMap::new();
    while !bytes.is_empty() {
        let name_end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(BundleError::UnsafePath)?;
        let name = std::str::from_utf8(&bytes[..name_end]).map_err(|_| BundleError::UnsafePath)?;
        bytes = &bytes[name_end + 1..];
        if bytes.len() < 8 {
            return Err(BundleError::FileMismatch);
        }
        let length = usize::try_from(u64::from_be_bytes(bytes[..8].try_into().unwrap()))
            .map_err(|_| BundleError::FileMismatch)?;
        bytes = &bytes[8..];
        if length > bytes.len()
            || files
                .insert(name.to_owned(), bytes[..length].to_vec())
                .is_some()
        {
            return Err(BundleError::FileMismatch);
        }
        bytes = &bytes[length..];
    }
    Ok(files)
}

fn validate_bundle(
    manifest_bytes: Vec<u8>,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<Bundle, BundleError> {
    if manifest_bytes.len() > MAX_APPROVED_MANIFEST_BYTES {
        return Err(BundleError::FileMismatch);
    }
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.bundle_version != BUNDLE_VERSION {
        return Err(BundleError::UnsupportedBundle);
    }
    if manifest.minimum_learning_step_schema != SUPPORTED_SCHEMA
        || manifest.entrypoint != "MAGNESIUM.md"
    {
        return Err(BundleError::UnsupportedSchema);
    }
    if manifest.status != "placeholder" && manifest.status != "active" {
        return Err(BundleError::ManifestMismatch);
    }
    let expected_paths: BTreeSet<String> = ALLOWED_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .chain(std::iter::once(MANIFEST.to_owned()))
        .collect();
    if files.keys().cloned().collect::<BTreeSet<_>>() != expected_paths {
        return Err(BundleError::ManifestMismatch);
    }
    if files.get(MANIFEST) != Some(&manifest_bytes) {
        return Err(BundleError::ManifestMismatch);
    }
    let mut seen = BTreeSet::new();
    for declared in &manifest.files {
        if !safe_relative_path(&declared.path) || !ALLOWED_PATHS.contains(&declared.path.as_str()) {
            return Err(BundleError::UnsafePath);
        }
        if !seen.insert(declared.path.as_str()) || !declared.required_for_activation {
            return Err(BundleError::ManifestMismatch);
        }
        let bytes = files.get(&declared.path).ok_or(BundleError::FileMismatch)?;
        if bytes.len() > MAX_APPROVED_ASSET_BYTES
            || sha256(bytes) != declared.sha256
            || !valid_role_metadata(declared)
        {
            return Err(BundleError::FileMismatch);
        }
        if manifest.status == "placeholder" && bytes.as_slice() != PLACEHOLDER_MARKER {
            return Err(BundleError::ManifestMismatch);
        }
    }
    if seen.len() != ALLOWED_PATHS.len() {
        return Err(BundleError::ManifestMismatch);
    }
    let bundle_hash = bundle_hash(&manifest, &manifest_bytes);
    Ok(Bundle {
        version: manifest.bundle_version,
        hash: bundle_hash,
        status: manifest.status,
        files: Arc::new(files),
    })
}

fn valid_role_metadata(file: &ManifestFile) -> bool {
    GENERATED_METADATA.iter().any(|(path, role, stage, host)| {
        file.path == *path
            && file.role == *role
            && file.stage.as_deref() == *stage
            && file.host.as_deref() == *host
    })
}

fn bundle_hash(manifest: &Manifest, manifest_bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"magnesium-instruction-bundle\0");
    digest.update(manifest.bundle_version.to_be_bytes());
    digest.update(manifest_bytes);
    for entry in &manifest.files {
        digest.update(entry.path.as_bytes());
        digest.update([0]);
        digest.update(entry.sha256.as_bytes());
        digest.update([0]);
    }
    hex(digest.finalize().as_slice())
}

fn sha256(bytes: &[u8]) -> String {
    hex(Sha256::digest(bytes).as_slice())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String");
    }
    output
}

fn safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && path.to_str() == Some(value)
        && !value.contains('\\')
}

/// Install the reviewed assets beneath `<control>/instructions/v2/<hash>`.
/// Placeholder assets are copied for inspection but never receive an active
/// pointer. A complete staged directory is validated before its atomic rename.
pub(crate) fn install(control: &Path) -> Result<InstallDisposition, String> {
    install_bundle(control).map_err(|error| error.to_string())
}

fn install_bundle(control: &Path) -> Result<InstallDisposition, BundleError> {
    let bundle = compiled_bundle()?;
    install_known_bundle(control, &bundle)
}

fn install_known_bundle(
    control: &Path,
    bundle: &Bundle,
) -> Result<InstallDisposition, BundleError> {
    ensure_real_directory(control)?;
    let instructions = control.join("instructions");
    create_or_validate_dir(&instructions)?;
    let v2 = instructions.join("v2");
    create_or_validate_dir(&v2)?;
    if materialize_bundle_dir(&v2, bundle)? {
        if bundle.status == "placeholder" {
            return Ok(InstallDisposition::VerifiedPassive);
        }
        return if activate_current_bundle(&instructions, bundle)? {
            Ok(InstallDisposition::UpdatedActive)
        } else {
            Ok(InstallDisposition::AlreadyActive)
        };
    }
    if bundle.status == "placeholder" {
        Ok(InstallDisposition::CreatedPassive)
    } else {
        activate_current_bundle(&instructions, bundle)?;
        Ok(InstallDisposition::CreatedAndActivated)
    }
}

/// Validate an existing `v2/<hash>` directory, or stage, validate and rename a
/// new one. Returns whether the directory already existed.
fn materialize_bundle_dir(v2: &Path, bundle: &Bundle) -> Result<bool, BundleError> {
    let target = v2.join(&bundle.hash);
    let target_exists = match fs::symlink_metadata(&target) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if target_exists {
        validate_installed_dir(&target, bundle)?;
        return Ok(true);
    }
    let stage = v2.join(format!(".stage-{}", crate::workspace::os_random_hex(12)?));
    crate::workspace::create_private_dir(&stage)?;
    let result = (|| {
        write_bundle_files(&stage, bundle)?;
        validate_installed_dir(&stage, bundle)?;
        fs::rename(&stage, &target)?;
        Ok::<(), BundleError>(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    result?;
    Ok(false)
}

/// Explicitly switch a workspace from an earlier reviewed bundle to the
/// compiled one. Implicit installs never replace an active pointer; this runs
/// only on request, only when the earlier bundle is byte-for-byte unedited
/// against its own manifest, and it keeps the earlier directory in place.
pub(crate) fn upgrade(control: &Path) -> Result<InstallDisposition, String> {
    upgrade_bundle(control).map_err(|error| match error {
        BundleError::UserEdits => format!(
            "{error}; the earlier instructions were edited locally, so they were left active and unchanged"
        ),
        error => error.to_string(),
    })
}

fn upgrade_bundle(control: &Path) -> Result<InstallDisposition, BundleError> {
    let bundle = compiled_bundle()?;
    upgrade_known_bundle(control, &bundle)
}

fn upgrade_known_bundle(
    control: &Path,
    bundle: &Bundle,
) -> Result<InstallDisposition, BundleError> {
    if bundle.status != "active" {
        return Err(BundleError::Placeholder);
    }
    let instructions = control.join("instructions");
    let pointer_path = instructions.join("active.json");
    let previous = match fs::symlink_metadata(&pointer_path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(BundleError::Symlink);
        }
        Ok(_) => {
            let bytes = read_installed_file(&pointer_path, ACTIVE_POINTER_LIMIT)?;
            let pointer: ActivePointer = serde_json::from_slice(&bytes)?;
            if bytes != serde_json::to_vec(&pointer)?
                || pointer.bundle_version != bundle.version
                || !is_bundle_hash(&pointer.bundle_hash)
            {
                return Err(BundleError::UserEdits);
            }
            Some(pointer.bundle_hash)
        }
    };
    let Some(previous) = previous.filter(|hash| *hash != bundle.hash) else {
        return install_known_bundle(control, bundle);
    };
    let v2 = instructions.join("v2");
    verify_unedited_bundle_dir(&v2.join(&previous), &previous)?;
    materialize_bundle_dir(&v2, bundle)?;
    let pointer = serde_json::to_vec(&ActivePointer {
        bundle_version: bundle.version,
        bundle_hash: bundle.hash.clone(),
    })?;
    let mut writes = crate::workspace::AtomicWrites::default();
    writes.replace(&pointer_path, |file| file.write_all(&pointer))?;
    writes.finish()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&pointer_path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(InstallDisposition::Upgraded { previous })
}

fn is_bundle_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// An earlier bundle is unedited when its directory holds exactly the files
/// its manifest declares, each with the declared hash, and the manifest
/// hashes to the directory's name. The current inventory rules do not apply:
/// earlier versions may have had other files.
fn verify_unedited_bundle_dir(directory: &Path, hash: &str) -> Result<(), BundleError> {
    validate_path_components(directory)?;
    let manifest_bytes =
        read_installed_file(&directory.join(MANIFEST), MAX_APPROVED_MANIFEST_BYTES)
            .map_err(|_| BundleError::UserEdits)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.bundle_version != BUNDLE_VERSION || bundle_hash(&manifest, &manifest_bytes) != hash
    {
        return Err(BundleError::UserEdits);
    }
    let declared: BTreeSet<String> = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .chain(std::iter::once(MANIFEST.to_owned()))
        .collect();
    if declared.iter().any(|path| !safe_relative_path(path))
        || enumerate_files(directory, &declared).map_err(|_| BundleError::UserEdits)? != declared
    {
        return Err(BundleError::UserEdits);
    }
    for file in &manifest.files {
        let bytes = read_installed_file(&directory.join(&file.path), MAX_APPROVED_ASSET_BYTES)?;
        if sha256(&bytes) != file.sha256 {
            return Err(BundleError::UserEdits);
        }
    }
    Ok(())
}

fn write_bundle_files(root: &Path, bundle: &Bundle) -> Result<(), BundleError> {
    let mut writes = crate::workspace::AtomicWrites::default();
    for (relative, bytes) in bundle.files.iter() {
        if !safe_relative_path(relative) {
            return Err(BundleError::UnsafePath);
        }
        let path = root.join(relative);
        let parent = path.parent().ok_or(BundleError::UnsafePath)?;
        crate::workspace::create_private_dir_all(parent)?;
        writes.replace(&path, |file| file.write_all(bytes))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
    }
    writes.finish()?;
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct ValidationWork {
    file_count: usize,
    bytes: usize,
}

impl ValidationWork {
    fn combine(self, other: Self) -> Self {
        Self {
            file_count: self.file_count.saturating_add(other.file_count),
            bytes: self.bytes.saturating_add(other.bytes),
        }
    }
}

fn validate_installed_dir(path: &Path, bundle: &Bundle) -> Result<ValidationWork, BundleError> {
    validate_path_components(path)?;
    let expected: BTreeSet<_> = bundle.files.keys().cloned().collect();
    let actual = enumerate_files(path, &expected)?;
    if actual != expected {
        return Err(BundleError::UserEdits);
    }
    let mut bytes_validated = 0usize;
    for (relative, expected_bytes) in bundle.files.iter() {
        let bytes = read_installed_file(&path.join(relative), expected_bytes.len())?;
        if &bytes != expected_bytes {
            return Err(BundleError::UserEdits);
        }
        bytes_validated = bytes_validated.saturating_add(bytes.len());
    }
    Ok(ValidationWork {
        file_count: actual.len(),
        bytes: bytes_validated,
    })
}

fn read_installed_file(path: &Path, limit: usize) -> Result<Vec<u8>, BundleError> {
    read_installed_file_after_metadata(path, limit, || {})
}

fn read_installed_file_after_metadata(
    path: &Path,
    limit: usize,
    after_metadata: impl FnOnce(),
) -> Result<Vec<u8>, BundleError> {
    let before = fs::symlink_metadata(path)?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(BundleError::Symlink);
    }
    after_metadata();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let file = {
        let descriptor = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(io::Error::from)?;
        fs::File::from(descriptor)
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let file = { fs::File::open(path)? };

    let opened = file.metadata()?;
    if !opened.is_file() || !same_file_identity(&opened, &before) {
        return Err(BundleError::Symlink);
    }
    let read_limit = u64::try_from(limit)
        .unwrap_or(u64::MAX - 1)
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    file.take(read_limit).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(BundleError::FileMismatch);
    }
    let after = fs::symlink_metadata(path)?;
    if after.file_type().is_symlink() || !after.is_file() || !same_file_identity(&opened, &after) {
        return Err(BundleError::UserEdits);
    }
    Ok(bytes)
}

#[cfg(unix)]
fn same_file_identity(opened: &fs::Metadata, path: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    opened.dev() == path.dev() && opened.ino() == path.ino()
}

#[cfg(not(unix))]
fn same_file_identity(opened: &fs::Metadata, path: &fs::Metadata) -> bool {
    opened.len() == path.len() && opened.modified().ok() == path.modified().ok()
}

/// Validate an installed tree against the finite compiled allowlist. Only the
/// directories named by allowed file prefixes are traversed, and the first
/// unknown file or directory fails: an arbitrary empty or nested tree cannot
/// create unbounded work.
fn enumerate_files(
    root: &Path,
    allowed: &BTreeSet<String>,
) -> Result<BTreeSet<String>, BundleError> {
    let mut allowed_dirs = BTreeSet::new();
    for path in allowed {
        let mut parts = path.split('/').peekable();
        let mut prefix = String::new();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            allowed_dirs.insert(prefix.clone());
        }
    }
    let max_entries = allowed.len().saturating_add(allowed_dirs.len());
    let mut found = BTreeSet::new();
    let mut visited = 0usize;
    let mut pending = vec![(root.to_path_buf(), String::new(), 0usize)];
    while let Some((directory, prefix, depth)) = pending.pop() {
        if depth > MAX_BUNDLE_DIRECTORY_DEPTH {
            return Err(BundleError::UnsafePath);
        }
        for entry in fs::read_dir(&directory)? {
            visited = visited.saturating_add(1);
            if visited > max_entries {
                return Err(BundleError::UnsafePath);
            }
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() || (!kind.is_file() && !kind.is_dir()) {
                return Err(BundleError::Symlink);
            }
            let file_name = entry.file_name();
            let name = file_name.to_str().ok_or(BundleError::UnsafePath)?;
            let relative = if prefix.is_empty() {
                name.to_owned()
            } else {
                format!("{prefix}/{name}")
            };
            if kind.is_dir() {
                if !allowed_dirs.contains(&relative) {
                    return Err(BundleError::UnsafePath);
                }
                pending.push((entry.path(), relative, depth + 1));
            } else if !allowed.contains(&relative)
                || !safe_relative_path(&relative)
                || !found.insert(relative)
            {
                return Err(BundleError::UnsafePath);
            }
        }
    }
    Ok(found)
}

fn validate_path_components(path: &Path) -> Result<(), BundleError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(BundleError::Symlink);
        }
    }
    Ok(())
}

fn ensure_real_directory(path: &Path) -> Result<(), BundleError> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(BundleError::Symlink);
    }
    validate_path_components(path)
}

fn create_or_validate_dir(path: &Path) -> Result<(), BundleError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(BundleError::Symlink)
        }
        Ok(_) => validate_path_components(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            crate::workspace::create_private_dir(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivePointer {
    bundle_version: u64,
    bundle_hash: String,
}

fn activate_current_bundle(instructions: &Path, bundle: &Bundle) -> Result<bool, BundleError> {
    if bundle.status != "active" {
        return Err(BundleError::Placeholder);
    }
    let pointer_path = instructions.join("active.json");
    let pointer = ActivePointer {
        bundle_version: bundle.version,
        bundle_hash: bundle.hash.clone(),
    };
    let bytes = serde_json::to_vec(&pointer)?;
    let pointer_metadata = match fs::symlink_metadata(&pointer_path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if let Some(metadata) = pointer_metadata {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(BundleError::Symlink);
        }
        let existing_bytes = read_installed_file(&pointer_path, ACTIVE_POINTER_LIMIT)?;
        let existing: ActivePointer = serde_json::from_slice(&existing_bytes)?;
        if existing.bundle_version == bundle.version
            && existing.bundle_hash != bundle.hash
            && is_bundle_hash(&existing.bundle_hash)
            && existing_bytes == serde_json::to_vec(&existing)?
        {
            return Err(BundleError::PreviousBundleActive);
        }
        if existing.bundle_version != bundle.version
            || existing.bundle_hash != bundle.hash
            || existing_bytes != bytes
        {
            return Err(BundleError::UserEdits);
        }
        return Ok(false);
    }
    let mut writes = crate::workspace::AtomicWrites::default();
    writes.replace(&pointer_path, |file| file.write_all(&bytes))?;
    writes.finish()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(pointer_path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(true)
}

fn load_installed(instructions: &Path, expected: &Bundle) -> Result<ValidationWork, BundleError> {
    validate_path_components(instructions)?;
    let pointer_path = instructions.join("active.json");
    let metadata = fs::symlink_metadata(&pointer_path).map_err(|_| BundleError::ActivePointer)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BundleError::Symlink);
    }
    let pointer_bytes = read_installed_file(&pointer_path, ACTIVE_POINTER_LIMIT)?;
    let pointer: ActivePointer = serde_json::from_slice(&pointer_bytes)?;
    if pointer.bundle_version != expected.version || pointer.bundle_hash != expected.hash {
        return Err(BundleError::ActivePointer);
    }
    if pointer_bytes != serde_json::to_vec(&pointer)? {
        return Err(BundleError::UserEdits);
    }
    // Validate on every load so local edits and symlinks remain visible. The
    // rendered bytes come from the immutable compile-time Arc, so no cloned
    // per-workspace copy or filesystem read is counted as avoided work.
    let directory = instructions.join("v2").join(&expected.hash);
    let pointer_validation = ValidationWork {
        file_count: 1,
        bytes: pointer_bytes.len(),
    };
    Ok(pointer_validation.combine(validate_installed_dir(&directory, expected)?))
}

pub(crate) fn load(
    control: &Path,
    stage: InstructionStage,
    host: InstructionHost,
    operation: Option<InstructionOperation>,
    capability_tier: &str,
) -> Value {
    let result = load_inner(control, stage, host, operation, capability_tier);
    match result {
        Ok(value) => serde_json::to_value(value).expect("instruction response serializes"),
        Err(error) => json!({
            "status": "unavailable",
            "error": { "code": error_code(&error), "message": error.to_string() },
            "recommended_stage": stage,
            "bundle_version": BUNDLE_VERSION,
            "bundle_hash": compiled_bundle().ok().map(|bundle| bundle.hash.clone()),
        }),
    }
}

fn load_inner(
    control: &Path,
    stage: InstructionStage,
    host: InstructionHost,
    operation: Option<InstructionOperation>,
    capability_tier: &str,
) -> Result<InstructionLoad, BundleError> {
    let expected = compiled_bundle()?;
    if expected.status == "placeholder" {
        return Err(BundleError::Placeholder);
    }
    let instructions = control.join("instructions");
    let validation = load_installed(&instructions, &expected)?;
    render(
        &expected,
        stage,
        host,
        operation,
        capability_tier,
        validation,
        true,
    )
}

fn render(
    bundle: &Bundle,
    stage: InstructionStage,
    host: InstructionHost,
    operation: Option<InstructionOperation>,
    capability_tier: &str,
    validation: ValidationWork,
    immutable_bytes_reused: bool,
) -> Result<InstructionLoad, BundleError> {
    let mut files = vec![
        "MAGNESIUM.md",
        "teaching-policy.md",
        host.asset(),
        stage.workflow(),
    ];
    if let Some(operation) = operation {
        files.push(operation.asset());
    }
    let mut text = String::new();
    let mut paths = Vec::new();
    let mut deferred_modules = Vec::new();
    for path in files {
        let bytes = bundle.files.get(path).ok_or(BundleError::FileMismatch)?;
        let section = std::str::from_utf8(bytes).map_err(|_| BundleError::FileMismatch)?;
        let separator_len = usize::from(!text.is_empty()) * 2;
        if text
            .len()
            .saturating_add(separator_len)
            .saturating_add(section.len())
            > RESPONSE_LIMIT
        {
            if path == stage.workflow()
                || path == "MAGNESIUM.md"
                || path == "teaching-policy.md"
                || path == host.asset()
            {
                return Err(BundleError::RequiredSetTooLarge);
            }
            deferred_modules.push(path.to_owned());
            continue;
        }
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(section);
        paths.push(path.to_owned());
    }
    Ok(InstructionLoad {
        status: "served",
        bundle_version: bundle.version,
        bundle_hash: bundle.hash.clone(),
        recommended_stage: stage,
        files: paths,
        deferred_modules,
        bytes_served: text.len(),
        validation_file_count: validation.file_count,
        validation_bytes: validation.bytes,
        immutable_bytes_reused,
        capability_tier: capability_tier.to_owned(),
        text,
    })
}

fn error_code(error: &BundleError) -> &'static str {
    match error {
        BundleError::UnsupportedBundle => "unsupported_bundle_version",
        BundleError::UnsupportedSchema => "unsupported_schema_version",
        BundleError::Placeholder => "placeholder_bundle",
        BundleError::ManifestMismatch => "bundle_mismatch",
        BundleError::FileMismatch => "file_hash_mismatch",
        BundleError::UnsafePath => "unsafe_path",
        BundleError::Symlink => "symlink_escape",
        BundleError::UserEdits => "unrecognized_user_edits",
        BundleError::PreviousBundleActive => "previous_bundle_active",
        BundleError::ActivePointer => "bundle_not_active",
        BundleError::RequiredSetTooLarge => "required_instruction_set_too_large",
        BundleError::Io(_) => "bundle_io_error",
        BundleError::Json(_) => "invalid_bundle_manifest",
    }
}

#[cfg(test)]
mod tests {

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
        assert_eq!(bundle.files.len(), 17);
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
            InstructionHost::Codex,
            None,
            "full_adapter",
        )
        .expect("approved instruction load");
        assert_eq!(loaded.status, "served");
        assert_eq!(loaded.bundle_hash, bundle.hash);
        assert!(loaded.immutable_bytes_reused);
        assert_eq!(loaded.validation_file_count, 18);
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
            "adapters\\codex.md",
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
        for host in [
            InstructionHost::Codex,
            InstructionHost::Claude,
            InstructionHost::Opencode,
        ] {
            assert!(
                super::GENERATED_METADATA
                    .iter()
                    .any(|(path, role, _, metadata_host)| {
                        *path == host.asset()
                            && *role == "adapter"
                            && *metadata_host == Some(host.manifest_key())
                    })
            );
        }
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
            for host in [
                InstructionHost::Codex,
                InstructionHost::Claude,
                InstructionHost::Opencode,
            ] {
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
            for host in [
                InstructionHost::Codex,
                InstructionHost::Claude,
                InstructionHost::Opencode,
            ] {
                for operation in [
                    None,
                    Some(InstructionOperation::StateAndEvidence),
                    Some(InstructionOperation::SourceUse),
                    Some(InstructionOperation::ToolRecipes),
                ] {
                    let loaded =
                        load_inner(&control, stage, host, operation, "instruction_only").unwrap();
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
                    assert_eq!(loaded.validation_file_count, 18);
                    assert_eq!(loaded.validation_bytes, validation_bytes);
                    assert!(loaded.immutable_bytes_reused);
                    let response =
                        super::load(&control, stage, host, operation, "instruction_only");
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
        }
        assert_eq!(served.len(), 16);
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
            for host in [
                InstructionHost::Codex,
                InstructionHost::Claude,
                InstructionHost::Opencode,
            ] {
                for operation in [
                    None,
                    Some(InstructionOperation::StateAndEvidence),
                    Some(InstructionOperation::SourceUse),
                    Some(InstructionOperation::ToolRecipes),
                ] {
                    let response =
                        super::load(&control, stage, host, operation, "instruction_only");
                    assert_eq!(response["status"], "unavailable");
                    assert_eq!(response["bundle_version"], bundle.version);
                    assert_eq!(response["bundle_hash"], bundle.hash);
                    assert_eq!(
                        response["recommended_stage"],
                        serde_json::to_value(stage).unwrap()
                    );
                    assert_eq!(response["error"]["code"], "bundle_not_active");
                    assert!(
                        response["error"]["message"].as_str().unwrap().contains(
                            "run `magnesium init --upgrade-instructions` in this workspace"
                        )
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
        }
        assert_eq!(fs::read_dir(&control).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&instructions).unwrap().count(), 0);
        fs::remove_dir_all(control).unwrap();
    }

    fn synthetic_bundle() -> super::Bundle {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/instruction_bundle");
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
        assert_eq!(first_validation.file_count, 18);
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
                InstructionHost::Codex,
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
                    "adapters/codex.md",
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
            assert_eq!(response.validation_file_count, 18);
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
                InstructionHost::Codex,
                None,
                "instruction_only",
            )
            .unwrap();
            let loaded_bundle = compiled_bundle().unwrap();
            assert!(Arc::ptr_eq(&loaded_bundle, &bundle));
            assert!(Arc::ptr_eq(&loaded_bundle.files, &bundle.files));
            assert!(loaded.immutable_bytes_reused);
            assert_eq!(loaded.bundle_hash, bundle.hash);
            // One active-pointer read plus the 16 instructions and manifest; the
            // ordinary path must not reopen the pointer it already verified.
            assert_eq!(loaded.validation_file_count, 18);
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
            InstructionHost::Opencode,
            None,
            "instruction_only",
            validation,
            false,
        )
        .unwrap();
        let with_operation = render(
            &bundle,
            InstructionStage::TeachAndPractice,
            InstructionHost::Opencode,
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
            ("adapters/codex.md", "adapters/browser.md"),
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
                InstructionHost::Codex,
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
            InstructionHost::Codex,
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
        assert_eq!(old_manifest["files"].as_array().unwrap().len(), 18);
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
        let fresh_validation =
            load_installed(&fresh_control.join("instructions"), &current).unwrap();
        assert_eq!(fresh_validation.file_count, 18);
        assert_eq!(current.files.len(), 17);
        assert_eq!(current.version, 2);
        fs::remove_dir_all(old_control).unwrap();
        fs::remove_dir_all(fresh_control).unwrap();
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
            18
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
            18
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
}
