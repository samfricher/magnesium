use super::helpers::resolve_workspace;
use super::types::SearchRequest;
use crate::retrieval::{
    CoordinatorBudget, FolderFiles, RetrievalRequest, SourceSearchRequest, folder_files,
    retrieve_workspace_sources,
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
    let resolved = resolve_workspace(cwd)?.ok_or("workspace_not_initialized")?;
    let root = resolved.workspace().root();
    let workspace_id = resolved.workspace_id().to_owned();
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
    let files = folder_files(&index_path)
        .ok()
        .map(|files| files_view(&files));
    Ok(
        json!({"schema":"learner-model/v2","workspace_id":workspace_id,"generation":report.generation,"coverage":report.coverage.as_str(),"reconciliation":report.reconciliation.diagnostic(),"hits":hits,"coverage_meaning":"bounded source indexing, not semantic completeness","files":files}),
    )
}

fn count(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn plural(value: usize, one: &str, many: &str) -> String {
    format!("{} {}", count(value), if value == 1 { one } else { many })
}

fn kind_words(kind: &str) -> &'static str {
    match kind {
        "image" => "images, which search cannot read; read them by eye",
        "document" => "office documents, which search cannot read; open them, or save them as PDF",
        "scan" => "PDFs with no text layer, such as scans; read them by eye",
        "no_titles" => {
            "PDFs with text but no section titles the runtime can find; read them by eye to name their sections"
        }
        "failed" => "files whose text could not be extracted",
        "name_word" => {
            "files left out because a word in the name looks like a password or key file"
        }
        "password_like" => "files left out because their text looks like it holds a password",
        "too_large" => "files too large to read",
        "link" => "links, which search does not follow",
        "unreadable" => "files that could not be opened",
        _ => "files of a kind search does not read",
    }
}

fn files_view(files: &FolderFiles) -> Value {
    let cannot = files
        .unreadable
        .iter()
        .filter(|group| !matches!(group.kind, "no_titles"))
        .map(|group| group.count)
        .sum::<usize>();
    let mut summary = if files.first_scan_done {
        format!(
            "{} of {} are indexed",
            count(files.indexed),
            plural(files.readable, "readable file", "readable files")
        )
    } else {
        format!(
            "The first look through this folder is still running: {} of the {} found so far are indexed",
            count(files.indexed),
            plural(files.readable, "readable file", "readable files")
        )
    };
    if files.waiting > 0 {
        summary.push_str(&format!("; {} waiting to be read", count(files.waiting)));
    }
    if files.rechecking && files.first_scan_done {
        summary.push_str("; the folder is being rechecked for changes");
    }
    summary.push('.');
    if cannot > 0 {
        summary.push_str(&format!(
            " {} cannot be read as text.",
            plural(cannot, "file", "files")
        ));
    }
    json!({
        "summary": summary,
        "indexed": files.indexed,
        "readable": files.readable,
        "waiting": files.waiting,
        "first_scan_done": files.first_scan_done,
        "rechecking": files.rechecking,
        "left_out_by_ignore_list": files.ignored,
        "cannot_read": files.unreadable.iter().map(|group| json!({
            "kind": group.kind,
            "what": kind_words(group.kind),
            "count": group.count,
            "folders": group.folders.iter().map(|(folder, files)| json!({"folder": folder, "files": files})).collect::<Vec<_>>(),
            "examples": group.examples,
        })).collect::<Vec<_>>(),
        "page_code": files.page_code.iter().map(|(folder, files)| json!({"folder": folder, "files": files})).collect::<Vec<_>>(),
        "page_code_meaning": "folders of web-page code and styles saved with the course pages, not course text; a folder listed in .magnesium/magnesiumignore is left out of search",
    })
}

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
