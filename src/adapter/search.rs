use super::helpers::resolve_workspace;
use super::types::SearchRequest;
use crate::retrieval::{
    CoordinatorBudget, RetrievalRequest, SourceSearchRequest, retrieve_workspace_sources,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(crate) fn handle_search(cwd: &Path, request: &SearchRequest) -> Result<Value, String> {
    if request.query.is_empty()
        || request.query.len() > 4096
        || request
            .limit
            .is_some_and(|n| n == 0 || n > crate::retrieval::MAX_SEARCH_RESULTS)
    {
        return Err("invalid source search bounds".into());
    }
    let (_, resolved) = resolve_workspace(cwd)?;
    let resolved = resolved.ok_or("workspace_not_initialized")?;
    let root = resolved.workspace().root();
    let workspace_id = resolved.workspace().id().map_err(|e| e.to_string())?;
    let index_path = crate::workspace::source_index_path(root);
    let report = retrieve_workspace_sources(
        root,
        &index_path,
        Path::new(""),
        &RetrievalRequest {
            active_workspace_id: workspace_id.clone(),
            lexical: SourceSearchRequest {
                query: request.query.clone(),
                path_prefix: request.path_prefix.as_deref().map(PathBuf::from),
                limit: request.limit.unwrap_or(10),
            },
        },
        CoordinatorBudget::for_search(&index_path),
    )
    .map_err(|e| e.to_string())?;
    let hits:Vec<_>=report.hits.iter().map(|hit|json!({
        "candidate_id":hit.candidate.candidate_id,"path":hit.candidate.path,
        "content_sha256":hit.candidate.content_sha256,"anchor_kind":hit.candidate.anchor_kind.as_str(),
        "anchor_start":hit.candidate.anchor_start,"anchor_end":hit.candidate.anchor_end,
        "snippet":source_snippet(&hit.candidate.body,&request.query),"workspace_id":workspace_id,
        "authority":"original_source"
    })).collect();
    Ok(
        json!({"schema":"learner-model/v2","workspace_id":workspace_id,"generation":report.generation,"coverage":report.coverage.as_str(),"reconciliation":report.reconciliation.diagnostic(),"hits":hits,"coverage_meaning":"bounded source indexing, not semantic completeness"}),
    )
}

/// Search previews stay small; `sources` opens the exact bounded anchor.
fn source_snippet(body: &str, query: &str) -> String {
    let terms: Vec<_> = query
        .split_whitespace()
        .map(|term| term.trim_matches('"').trim_end_matches('*').to_lowercase())
        .filter(|term| !term.is_empty())
        .collect();
    let line = body
        .lines()
        .position(|line| {
            let lower = line.to_lowercase();
            terms.iter().any(|term| lower.contains(term))
        })
        .unwrap_or(0);
    body.lines()
        .skip(line.saturating_sub(1))
        .flat_map(|line| line.chars().chain(std::iter::once('\n')))
        .take(1200)
        .collect()
}
