use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use same_file::Handle;

use super::{ManifestError, io_error};
use crate::retrieval::{
    DiscoveryDisposition, DiscoveryEntry, ExclusionReason, SensitivityClass, SourceKind,
};
use crate::workspace::decode_native_path;

pub(crate) fn manifest_path_is_missing(root: &Path, path_key: &[u8]) -> bool {
    let Ok(relative) = decode_native_path(path_key) else {
        return false;
    };
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return false;
    }
    matches!(
        fs::symlink_metadata(root.join(relative)),
        Err(error) if error.kind() == io::ErrorKind::NotFound
    )
}

pub(crate) fn source_kind(entry: &DiscoveryEntry) -> Option<&'static str> {
    match entry.disposition {
        DiscoveryDisposition::Source(SourceKind::Text) => Some("text"),
        DiscoveryDisposition::Source(SourceKind::Pdf) => Some("pdf"),
        DiscoveryDisposition::Excluded(_) => None,
    }
}

pub(crate) fn sensitivity(entry: &DiscoveryEntry) -> Option<&'static str> {
    match entry.disposition {
        DiscoveryDisposition::Excluded(ExclusionReason::Sensitive(class)) => Some(match class {
            SensitivityClass::Environment => "environment",
            SensitivityClass::Credential => "credential",
            SensitivityClass::PrivateKey => "private_key",
        }),
        _ => None,
    }
}

pub(crate) fn exclusion_reason(entry: &DiscoveryEntry) -> Option<&'static str> {
    match entry.disposition {
        DiscoveryDisposition::Excluded(ExclusionReason::BuiltIn) => Some("built_in"),
        DiscoveryDisposition::Excluded(ExclusionReason::Magnesiumignore) => Some("magnesiumignore"),
        DiscoveryDisposition::Excluded(ExclusionReason::NestedRepository) => {
            Some("nested_repository")
        }
        DiscoveryDisposition::Excluded(ExclusionReason::PrivateExport) => Some("private_export"),
        DiscoveryDisposition::Excluded(ExclusionReason::Sensitive(_)) => Some("sensitive"),
        DiscoveryDisposition::Excluded(ExclusionReason::Symlink) => Some("symlink"),
        DiscoveryDisposition::Excluded(ExclusionReason::TooLarge { .. }) => Some("too_large"),
        DiscoveryDisposition::Excluded(ExclusionReason::Unsupported) => Some("unsupported"),
        DiscoveryDisposition::Excluded(ExclusionReason::Unreadable(_)) => Some("unreadable"),
        DiscoveryDisposition::Excluded(ExclusionReason::EscapesWorkspace) => {
            Some("escapes_workspace")
        }
        DiscoveryDisposition::Source(_) => None,
    }
}

pub(crate) fn modified_ns(path: &Path) -> Option<i64> {
    fs::symlink_metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| i64::try_from(duration.as_nanos()).unwrap_or(i64::MAX))
}

pub(crate) fn required_modified_ns(
    path: &Path,
    metadata: &fs::Metadata,
) -> Result<i64, ManifestError> {
    let modified = metadata
        .modified()
        .map_err(|source| io_error("read source modification time", path, source))?;
    let nanos = modified
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ManifestError::Invalid(format!("{} predates the Unix epoch", path.display())))?
        .as_nanos();
    Ok(i64::try_from(nanos).unwrap_or(i64::MAX))
}

pub(crate) struct OpenSource {
    pub(crate) file: fs::File,
    pub(crate) metadata: fs::Metadata,
    pub(crate) path: PathBuf,
}

pub(crate) fn open_workspace_source(
    root: &Path,
    relative: &Path,
    after_open: &mut dyn FnMut(&Path) -> Result<(), ManifestError>,
) -> Result<OpenSource, ManifestError> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ManifestError::Invalid(format!(
            "source path {} is not workspace-relative",
            relative.display()
        )));
    }
    let canonical_root = fs::canonicalize(root)
        .map_err(|source| io_error("resolve workspace root", root, source))?;
    let path = canonical_root.join(relative);
    let metadata =
        fs::symlink_metadata(&path).map_err(|source| io_error("inspect source", &path, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ManifestError::Invalid(format!(
            "source {} must be a regular non-symlink file",
            path.display()
        )));
    }
    let file = fs::File::open(&path).map_err(|source| io_error("open source", &path, source))?;
    after_open(&path)?;
    let metadata = fs::symlink_metadata(&path)
        .map_err(|source| io_error("reinspect opened source", &path, source))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ManifestError::Invalid(format!(
            "source {} changed while opening and is not a regular non-symlink file",
            path.display()
        )));
    }
    let canonical =
        fs::canonicalize(&path).map_err(|source| io_error("resolve source", &path, source))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(ManifestError::Invalid(format!(
            "source {} escapes workspace {}",
            canonical.display(),
            canonical_root.display()
        )));
    }
    let opened_metadata = file
        .metadata()
        .map_err(|source| io_error("inspect opened source", &path, source))?;
    let opened_handle = Handle::from_file(
        file.try_clone()
            .map_err(|source| io_error("clone opened source handle", &path, source))?,
    )
    .map_err(|source| io_error("identify opened source", &path, source))?;
    let named_handle = Handle::from_path(&canonical)
        .map_err(|source| io_error("identify resolved source", &canonical, source))?;
    if opened_handle != named_handle {
        return Err(ManifestError::Invalid(format!(
            "source {} changed while opening",
            path.display()
        )));
    }
    Ok(OpenSource {
        file,
        metadata: opened_metadata,
        path: canonical,
    })
}
