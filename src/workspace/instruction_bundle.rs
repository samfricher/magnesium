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
/// Caps the instruction text in one load. Across every stage, the MCP text
/// block adds at most about 4% of JSON escapes and 477 bytes of fields, so
/// 32 KiB of text makes an MCP text block of at most about 34,600 bytes. The
/// binding host limit is higher: Claude Code saves a result over 50,000
/// characters to a file.
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
    Claude,
}

impl InstructionHost {
    fn manifest_key(self) -> &'static str {
        match self {
            Self::Claude => "claude",
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

/// Whether this workspace's active teaching instructions are an earlier
/// Magnesium version's, which `magnesium init --upgrade-instructions`
/// switches. It reads the active pointer only and changes nothing, so a
/// caller can ask before the switch.
pub(crate) fn earlier_bundle_active(control: &Path) -> bool {
    compiled_bundle().is_ok_and(|bundle| earlier_known_bundle_active(control, &bundle))
}

fn earlier_known_bundle_active(control: &Path, bundle: &Bundle) -> bool {
    if bundle.status != "active" {
        return false;
    }
    let Ok(bytes) = read_installed_file(
        &control.join("instructions/active.json"),
        ACTIVE_POINTER_LIMIT,
    ) else {
        return false;
    };
    // The same test `activate_current_bundle` makes before it refuses with
    // `PreviousBundleActive`.
    serde_json::from_slice::<ActivePointer>(&bytes).is_ok_and(|pointer| {
        pointer.bundle_version == bundle.version
            && pointer.bundle_hash != bundle.hash
            && is_bundle_hash(&pointer.bundle_hash)
            && serde_json::to_vec(&pointer).is_ok_and(|canonical| canonical == bytes)
    })
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
mod tests;
