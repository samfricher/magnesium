//! Retrieval for the active workspace only. Results preserve source provenance but
//! grant no authority to mutate learner state, invoke tools, or change policy.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::coordinator::{CoordinatorBudget, OperationCancellation};
use super::manifest::ManifestError;
use super::search::{
    SourceSearchReport, SourceSearchRequest,
    search_workspace_sources_with_allowlist_and_cancellation,
};
use crate::workspace::Workspace;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RetrievalRequest {
    pub(crate) active_workspace_id: String,
    pub(crate) lexical: SourceSearchRequest,
}

pub(crate) fn retrieve_workspace_sources(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &RetrievalRequest,
    budget: CoordinatorBudget,
) -> Result<SourceSearchReport, ManifestError> {
    retrieve_workspace_sources_with_allowlist_and_cancellation(
        workspace_root,
        index_path,
        query_scope,
        request,
        budget,
        None,
        &OperationCancellation::none(),
    )
}

pub(crate) fn retrieve_workspace_sources_with_allowlist_and_cancellation(
    workspace_root: &Path,
    index_path: &Path,
    query_scope: &Path,
    request: &RetrievalRequest,
    budget: CoordinatorBudget,
    allowed_source_paths: Option<&BTreeSet<PathBuf>>,
    cancellation: &OperationCancellation,
) -> Result<SourceSearchReport, ManifestError> {
    verify_active_workspace(workspace_root, &request.active_workspace_id)?;
    cancellation.check()?;
    search_workspace_sources_with_allowlist_and_cancellation(
        workspace_root,
        index_path,
        query_scope,
        &request.lexical,
        budget,
        allowed_source_paths,
        cancellation,
    )
}

fn verify_active_workspace(
    workspace_root: &Path,
    active_workspace_id: &str,
) -> Result<(), ManifestError> {
    let workspace = Workspace::discover(workspace_root)
        .map_err(|source| ManifestError::Io {
            action: "resolve active retrieval workspace",
            path: workspace_root.to_path_buf(),
            source,
        })?
        .ok_or_else(|| {
            ManifestError::Invalid(format!(
                "retrieval workspace {} is not initialized",
                workspace_root.display()
            ))
        })?;
    let actual_workspace_id = workspace.id().map_err(|source| ManifestError::Io {
        action: "read active retrieval workspace identity",
        path: workspace.control().to_path_buf(),
        source,
    })?;
    if actual_workspace_id != active_workspace_id {
        return Err(ManifestError::Invalid(format!(
            "retrieval workspace identity `{active_workspace_id}` does not match active workspace `{actual_workspace_id}`"
        )));
    }
    Ok(())
}
