use crate::workspace::{ResolvedWorkspace, WorkspaceResolution, resolve_initialized};
use std::path::{Path, PathBuf};
pub(crate) fn resolve_workspace(
    cwd: &Path,
) -> Result<(PathBuf, Option<ResolvedWorkspace>), String> {
    let resolution = resolve_initialized(cwd).map_err(|e| e.to_string())?;
    let workspace = match resolution {
        WorkspaceResolution::Active(workspace) => Some(workspace),
        WorkspaceResolution::Inactive => None,
    };
    Ok((cwd.to_path_buf(), workspace))
}
