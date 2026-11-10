use crate::workspace::{ResolvedWorkspace, WorkspaceResolution, resolve_initialized};
use std::path::Path;
pub(crate) fn resolve_workspace(cwd: &Path) -> Result<Option<ResolvedWorkspace>, String> {
    let resolution = resolve_initialized(cwd).map_err(|e| e.to_string())?;
    Ok(match resolution {
        WorkspaceResolution::Active(workspace) => Some(workspace),
        WorkspaceResolution::Inactive => None,
    })
}
