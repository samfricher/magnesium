//! Resolve learner ownership exclusively from the nearest native workspace.
use super::{CONTROL_DIR, Workspace, config::WorkspaceConfig};
use crate::learner::store::LearnerStore;
use std::path::{Path, PathBuf};
use std::{fs, io};

#[derive(Clone, Debug)]
pub(crate) struct ResolvedWorkspace {
    workspace: Workspace,
    workspace_id: String,
    store_path: PathBuf,
}
impl ResolvedWorkspace {
    pub(crate) fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    pub(crate) fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    pub(crate) fn store_path(&self) -> &Path {
        &self.store_path
    }
}
#[derive(Clone, Debug)]
pub(crate) enum WorkspaceResolution {
    Active(ResolvedWorkspace),
    Inactive,
}

pub(crate) fn resolve_initialized(start: &Path) -> io::Result<WorkspaceResolution> {
    let Some(workspace) = Workspace::discover(start)? else {
        return Ok(WorkspaceResolution::Inactive);
    };
    let config = WorkspaceConfig::load(&workspace.control().join("config.toml"))
        .map_err(io::Error::other)?;
    let workspace_id = config.workspace_id().to_owned();
    let store_path = workspace.store_path();
    super::reject_symlink_components(&store_path, workspace.control())?;
    LearnerStore::open(&store_path, &workspace_id).map_err(io::Error::other)?;
    Ok(WorkspaceResolution::Active(ResolvedWorkspace {
        workspace,
        workspace_id,
        store_path,
    }))
}

pub(super) fn nearest_boundary(start: &Path) -> io::Result<Option<PathBuf>> {
    let metadata = fs::metadata(start)?;
    let directory = if metadata.is_dir() {
        start
    } else {
        start.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file has no parent directory")
        })?
    };
    let mut current = fs::canonicalize(directory)?;
    loop {
        let control = current.join(CONTROL_DIR);
        match fs::symlink_metadata(&control) {
            Ok(metadata) if metadata.file_type().is_dir() => return Ok(Some(current)),
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Magnesium control path {} must be a real directory, not a file or symlink",
                        control.display()
                    ),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests;
