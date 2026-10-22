//! Original-source inspection with trustworthy anchors (LEARN-206).
//!
//! The `sources` domain operation exposes the exact cited physical PDF page
//! or text/code anchor alongside the extracted excerpt, using the original
//! file version. It returns file identity/hash, page/line location, source
//! authority scope, extraction method/version, coverage and any
//! missing/unreadable portions. Opening a source for inspection creates no
//! learner state: this handler performs no canonical-store write, promotes
//! no concept, and imports no book into model context (bounded excerpt plus
//! bounded original only).
//!
//! Physical page indices are used internally with explicit display labels;
//! printed page numbers are never silently treated as file-page indices. An
//! extracted formula is text-layer output only, never certified correct.
//! Empty/image-only or suspicious extraction yields `text_unavailable` or
//! `partial` plus an original-page option, never 'no relevant content
//! exists'. The runtime never claims visual analysis from returned text.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

#[cfg(test)]
use rusqlite::Connection;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::helpers::resolve_workspace;
use super::types::{SourceAction, SourcesRequest};
#[cfg(test)]
use crate::retrieval::manifest_test_facts::{
    load_current_candidates, read_manifest_row, read_manifest_row_from,
};
use crate::retrieval::{
    CandidateFacts, FactsInterest, Interval, MAX_PDF_SOURCE_BYTES, ManifestError, ManifestRow,
    OperationCancellation, SOURCES_EXCERPT_CHAR_CAP, open_workspace_source,
    prepare_explicit_source,
};
#[cfg(test)]
use crate::retrieval::{
    CoordinatorBudget, RetrievalRequest, SourceSearchRequest, SpanFact, retrieve_workspace_sources,
};
use crate::workspace::ResolvedWorkspace;

/// Bounded original/excerpt caps for one `sources` response. The excerpt cap
/// matches the search snippet bound; the original cap keeps one anchor's raw
/// bytes inspectable without importing a whole book into model context.
pub(crate) const SOURCES_ORIGINAL_BYTE_CAP: usize = 64 * 1024;
/// Text sources above this size are excluded at discovery (`too_large`); the
/// value mirrors `retrieval::MAX_SOURCE_BYTES` for status reporting.
const SOURCES_TEXT_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
/// Native extraction provenance labels. PDF text comes from the isolated
/// five-second `pdf_extract` worker (`pdf-extract 0.12.0`); that is text-layer
/// extraction, not layout/figure/formula understanding.
const TEXT_EXTRACTION_METHOD: &str = "native-text/v1";
const PDF_EXTRACTION_METHOD: &str = "native-pdf-worker/v1";

pub(crate) fn handle_sources(cwd: &Path, request: &SourcesRequest) -> Result<Value, String> {
    let action = request.action;
    let schema_version = request.schema_version;
    if let Some(version) = schema_version
        && version != 1
    {
        return Err(format!("unsupported MCP schema version: {version}"));
    }
    let (_cwd, workspace) = resolve_workspace(cwd)?;
    let Some(workspace) = workspace else {
        return Ok(json!({
            "schema_version": 1,
            "action": action,
            "active": false,
            "outcome": "inactive",
        }));
    };
    sources_resolved(&workspace, action, request)
}

struct SourceAnchorRequest {
    path: String,
    relative: PathBuf,
    expected_hash: Option<String>,
    kind: Option<String>,
    start: u32,
    end: Option<u32>,
}

struct Coverage {
    status: &'static str,
    issues: Vec<(Interval, &'static str)>,
    unknown_tail: bool,
    empty_source: bool,
}

fn sources_resolved(
    resolved: &ResolvedWorkspace,
    action: SourceAction,
    reference: &SourcesRequest,
) -> Result<Value, String> {
    let workspace_id = resolved.workspace_id().to_owned();
    let root = resolved.workspace().root().to_owned();
    if is_external_reference(reference.path.as_deref()) {
        return Ok(external_citation(
            action,
            reference.path.as_deref().unwrap_or(""),
        ));
    }
    let mut anchor = resolve_anchor(reference)?;
    // Reconciliation is bounded and may leave a previously indexed row in
    // place. Enforce current source policy independently before inspecting
    // either cached excerpts or original bytes, with or without a hash.
    if crate::retrieval::source_relative_path_is_excluded(&root, &anchor.relative)
        .map_err(|error| error.to_string())?
    {
        return Ok(json!({
            "schema_version": 1,
            "action": action,
            "active": true,
            "file": {"path": anchor.path},
            "anchor": anchor_view(&anchor, None),
            "authority": {"workspace_id": workspace_id, "type": "original_source"},
            "extraction": extraction_view("unknown", None),
            "coverage": coverage_view("excluded", Vec::new(), Some("source is excluded by current discovery policy and authorizes no bytes")),
            "basis": basis_view(),
        }));
    }
    let cancellation = OperationCancellation::none();
    let prepared = prepare_explicit_source(
        &root,
        &anchor.relative,
        FactsInterest::Range {
            start: anchor.start,
            end: anchor.end,
            include_excerpt: action.includes_content(),
        },
        &cancellation,
    )
    .map_err(|error| error.to_string())?;
    let preparation = prepared.preparation;
    let row = prepared.row;
    let facts = prepared.candidates;
    let mut view = (|| {
        let Some(row) = row else {
            return Ok(json!({
                "schema_version": 1,
                "action": action,
                "active": true,
                "file": {"path": anchor.path},
                "anchor": anchor_view(&anchor, None),
                "authority": {"workspace_id": workspace_id, "type": "original_source"},
                "extraction": extraction_view("unknown", None),
                "coverage": coverage_view("unknown", Vec::new(), Some("source is not a current indexed source; add the file to the workspace normally and retry")),
                "basis": basis_view(),
            }));
        };
        if !row_present(&row) {
            return Ok(state_view(
                action,
                &workspace_id,
                &anchor,
                &row,
                "deleted",
                Some("source was deleted and authorizes no bytes"),
            ));
        }
        if row_excluded(&row) {
            return Ok(state_view(
                action,
                &workspace_id,
                &anchor,
                &row,
                "excluded",
                Some(format!(
                    "source is excluded ({}) and authorizes no bytes",
                    row.exclusion_reason.as_deref().unwrap_or("excluded")
                )),
            ));
        }
        if row.source_kind != "text" && row.source_kind != "pdf" {
            return Ok(unsupported_view(action, &workspace_id, &anchor, &row));
        }
        let current_hash = row.content_sha256.clone().unwrap_or_default();
        if let Some(expected) = anchor.expected_hash.as_deref()
            && expected != current_hash
        {
            let mut view = state_view(
                action,
                &workspace_id,
                &anchor,
                &row,
                "stale",
                Some(
                    "source changed after the cited version; rerun search and retry with the current anchor",
                ),
            );
            view["current_content_sha256"] = json!(current_hash);
            return Ok(view);
        }
        let expected_kind = if row.source_kind == "pdf" {
            "page"
        } else {
            "line"
        };
        if anchor
            .kind
            .as_deref()
            .is_some_and(|kind| kind != expected_kind)
        {
            return Err(format!(
                "incompatible anchor_kind: {} sources require {expected_kind} anchors",
                row.source_kind
            ));
        }
        anchor.kind = Some(expected_kind.to_owned());
        if row.index_status == "failed"
            && !matches!(
                preparation,
                crate::retrieval::PreparationDisposition::PdfContentFailure
            )
        {
            return Ok(state_view(
                action,
                &workspace_id,
                &anchor,
                &row,
                "partial",
                Some(
                    "source extraction is incomplete; preparation did not publish a PDF content failure",
                ),
            ));
        }
        open_current_source(action, &workspace_id, &root, &anchor, &row, facts)
    })()?;
    view["preparation"] = json!(preparation);
    Ok(view)
}

fn is_external_reference(path: Option<&str>) -> bool {
    path.is_some_and(|value| value.contains("://"))
}

fn external_citation(action: SourceAction, reference: &str) -> Value {
    json!({
        "schema_version": 1,
        "action": action,
        "active": true,
        "authority": {
            "type": "external_citation",
            "detail": "model-reported or host-provided reference; not a locally fetched and hashed original",
        },
        "citation": reference.chars().take(512).collect::<String>(),
        "coverage": coverage_view("unknown", Vec::new(), Some("external material is never fetched automatically")),
        "snapshot_guidance": "to make external material durable, add the downloaded file to the workspace normally or explicitly authorize saving an accessible snapshot into the workspace's ordinary source directory; a quoted URL or hash alone is never native source verification and never bypasses paywalls",
        "basis": basis_view(),
    })
}

fn resolve_anchor(reference: &SourcesRequest) -> Result<SourceAnchorRequest, String> {
    if let Some(path) = reference.path.as_deref() {
        if let Some(hash) = reference.content_sha256.as_deref()
            && !is_lowercase_sha256(hash)
        {
            return Err("invalid content_sha256: expected 64 lowercase hex characters".to_owned());
        }
        let kind = match reference.anchor_kind.as_deref() {
            None => None,
            Some(raw) => Some(
                prepared_anchor_kind(raw)
                    .ok_or_else(|| "invalid anchor_kind: expected line or page".to_owned())?,
            ),
        };
        let (start, end) = match (reference.anchor_start, reference.anchor_end) {
            (None, None) => (1, None),
            (Some(start), None) => (start, None),
            (None, Some(_)) => {
                return Err("anchor_end without anchor_start is ambiguous".to_owned());
            }
            (Some(start), Some(end)) => (start, Some(end)),
        };
        if start == 0 || end.is_some_and(|end| end < start) {
            return Err("invalid anchor range: expected 1-based start <= end".to_owned());
        }
        let relative = validated_relative_path(path)?;
        Ok(SourceAnchorRequest {
            path: path.to_owned(),
            relative,
            expected_hash: reference.content_sha256.clone(),
            kind,
            start,
            end,
        })
    } else {
        Err("sources reference must supply a workspace-relative path".to_owned())
    }
}

fn validated_relative_path(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() || path.chars().count() > 256 || path.chars().any(char::is_control) {
        return Err(
            "invalid source path: expected 1-256 chars without control characters".to_owned(),
        );
    }
    let relative = PathBuf::from(path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(
            "invalid source path: expected a workspace-relative path without traversal".to_owned(),
        );
    }
    Ok(relative)
}

fn row_present(row: &ManifestRow) -> bool {
    // Deleted rows keep their last known hash/kind for truthful reporting
    // but must never authorize bytes; the caller checks this first.
    row.present && row.index_status != "deleted"
}

fn row_excluded(row: &ManifestRow) -> bool {
    row.source_kind == "excluded" || row.index_status == "excluded"
}

fn state_view(
    action: SourceAction,
    workspace_id: &str,
    anchor: &SourceAnchorRequest,
    row: &ManifestRow,
    coverage: &str,
    detail: Option<impl Into<String>>,
) -> Value {
    json!({
        "schema_version": 1,
        "action": action,
        "active": true,
        "file": {
            "path": row.display_path,
            "content_sha256": row.content_sha256,
            "file_size": row.file_size,
            "source_kind": row.source_kind,
        },
        "anchor": anchor_view(anchor, None),
        "authority": {"workspace_id": workspace_id, "type": "original_source"},
        "extraction": extraction_view(&row.source_kind, Some(row)),
        "coverage": coverage_view(coverage, Vec::new(), detail.map(Into::into).as_deref()),
        "basis": basis_view(),
    })
}

fn unsupported_view(
    action: SourceAction,
    workspace_id: &str,
    anchor: &SourceAnchorRequest,
    row: &ManifestRow,
) -> Value {
    json!({
        "schema_version": 1,
        "action": action,
        "active": true,
        "file": {
            "path": row.display_path,
            "content_sha256": row.content_sha256,
            "file_size": row.file_size,
            "source_kind": row.source_kind,
        },
        "anchor": anchor_view(anchor, None),
        "authority": {"workspace_id": workspace_id, "type": "original_source"},
        "extraction": extraction_view(&row.source_kind, Some(row)),
        "coverage": coverage_view("unsupported", Vec::new(), Some("Magnesium indexes text and PDF sources only")),
        "alternative": "open the file with its native application; Magnesium cannot extract or anchor this modality and makes no source-supported claim about its contents",
        "basis": basis_view(),
    })
}

fn anchor_view(anchor: &SourceAnchorRequest, heading: Option<&str>) -> Value {
    json!({
        "kind": anchor.kind,
        "physical_start": anchor.start,
        "physical_end": anchor.end,
        "display_label": anchor.kind.as_deref().map(|kind| display_label(kind, anchor.start, anchor.end, heading)),
        "page_numbering": "physical_file_index_1_based; printed page numbers may differ and must never be substituted silently",
        "heading": heading,
    })
}

fn display_label(kind: &str, start: u32, end: Option<u32>, heading: Option<&str>) -> String {
    let base = if let Some(end) = end {
        if kind == "page" {
            if start == end {
                format!("physical page {start}")
            } else {
                format!("physical pages {start}-{end}")
            }
        } else if start == end {
            format!("line {start}")
        } else {
            format!("lines {start}-{end}")
        }
    } else {
        if kind == "page" {
            format!("physical pages {start} to end")
        } else {
            format!("lines {start} to end")
        }
    };
    match heading {
        Some(heading) if !heading.is_empty() => format!("{base} ({heading})"),
        _ => base,
    }
}

fn converter_view(row: &ManifestRow) -> Value {
    let (Some(method), Some(method_version)) = (
        row.extraction_method.as_deref(),
        row.extraction_method_version.as_deref(),
    ) else {
        return Value::Null;
    };
    let strings = |raw: Option<&str>| {
        raw.and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
            .unwrap_or_default()
    };
    json!({
        "method": method,
        "method_version": method_version,
        "coverage": row.extraction_coverage,
        "missing_ranges": strings(row.extraction_missing_json.as_deref()),
        "warnings": strings(row.extraction_warnings_json.as_deref()),
        "note": "derived-converter output for this generation; verify uncertain regions against the original page",
    })
}

fn extraction_view(source_kind: &str, row: Option<&ManifestRow>) -> Value {
    let converter = row.map(converter_view).unwrap_or(Value::Null);
    let (method, version, note) = match (source_kind, converter.is_null()) {
        (_, false) => (
            row.and_then(|row| row.extraction_method.clone())
                .unwrap_or_default(),
            row.and_then(|row| row.extraction_method_version.clone())
                .unwrap_or_default(),
            "derived-converter output: text-layer extraction only, plus converter uncertainty in `converter`; figures, formulas, scanned pages and reading order are not guaranteed understood",
        ),
        ("pdf", _) => (
            PDF_EXTRACTION_METHOD.to_owned(),
            "pdf-extract 0.12.0 via isolated 5-second worker".to_owned(),
            "text-layer extraction only; figures, formulas, scanned pages and reading order are not guaranteed understood",
        ),
        _ => (
            TEXT_EXTRACTION_METHOD.to_owned(),
            "native-text/v1".to_owned(),
            "text-layer extraction only; figures, formulas, scanned pages and reading order are not guaranteed understood",
        ),
    };
    json!({
        "method": method,
        "version": version,
        "text_layer_only": true,
        "note": note,
        "converter": converter,
        "enhanced_extraction": crate::retrieval::conversion_worker_status(),
        "limits": {
            "max_text_source_bytes": SOURCES_TEXT_LIMIT_BYTES,
            "max_pdf_source_bytes": MAX_PDF_SOURCE_BYTES as u64,
            "max_pdf_pages": 2048,
            "max_pdf_text_bytes": 8 * 1024 * 1024,
            "pdf_deadline_secs": 5,
            "response_excerpt_chars": SOURCES_EXCERPT_CHAR_CAP,
            "response_original_bytes": SOURCES_ORIGINAL_BYTE_CAP,
        },
    })
}

fn coverage_view(status: &str, missing_ranges: Vec<String>, last_error: Option<&str>) -> Value {
    json!({
        "status": status,
        "missing_ranges": missing_ranges,
        "last_error": last_error,
    })
}

fn basis_view() -> Value {
    json!({
        "visual_analysis": false,
        "note": "No visual analysis was performed.",
    })
}

fn open_current_source(
    action: SourceAction,
    workspace_id: &str,
    root: &Path,
    anchor: &SourceAnchorRequest,
    row: &ManifestRow,
    facts: CandidateFacts,
) -> Result<Value, String> {
    let current_hash = row.content_sha256.as_deref().unwrap_or_default();
    let known_pdf_extent = (row.source_kind == "pdf"
        && row.index_status == "indexed"
        && row.extraction_method.is_none()
        && row.extraction_method_version.is_none()
        && row.extraction_coverage.is_none())
    .then_some(facts.observed_end);
    let (text_extent, original) =
        read_bounded_original(root, anchor, row, current_hash, action, known_pdf_extent)?;
    if crate::retrieval::source_relative_path_is_excluded(root, &anchor.relative)
        .map_err(|error| error.to_string())?
    {
        return Ok(state_view(
            action,
            workspace_id,
            anchor,
            row,
            "excluded",
            Some("source is excluded by current discovery policy and authorizes no bytes"),
        ));
    }
    OperationCancellation::none()
        .check()
        .map_err(|error| error.to_string())?;
    let known_extent = text_extent.or(known_pdf_extent);
    let coverage = classify_coverage(anchor, known_extent, &facts, row);
    let kind = anchor.kind.as_deref().expect("supported source kind");
    let mut view = state_view(
        action,
        workspace_id,
        anchor,
        row,
        coverage.status,
        row.last_error.as_deref(),
    );
    view["anchor"] = anchor_view(anchor, facts.heading.as_deref());
    let missing = coverage
        .issues
        .iter()
        .map(|(span, _)| format!("{kind} {}-{}", span.start, span.end))
        .collect();
    view["coverage"] = coverage_view(coverage.status, missing, row.last_error.as_deref());
    view["coverage"]["range_issues"] = json!(coverage.issues.iter().map(|(span, reason)| json!({"kind":kind,"start":span.start,"end":span.end,"reason":reason})).collect::<Vec<_>>());
    view["coverage"]["unknown_tail"] = json!(coverage.unknown_tail);
    view["coverage"]["empty_source"] = json!(coverage.empty_source);
    if known_extent.is_none() {
        view["coverage"]["extent_note"] = json!(
            "physical PDF extent is unknown; extracted anchors do not establish the file's end"
        );
    } else if coverage.empty_source {
        view["coverage"]["extent_note"] =
            json!("the verified source contains no physical lines or pages");
    }
    if let Some(excerpt) = facts.excerpt {
        view["excerpt"] = json!(excerpt);
        view["excerpt_truncated"] = json!(facts.excerpt_truncated);
        view["excerpt_ranges"] = json!(
            facts
                .excerpt_ranges
                .iter()
                .map(|span| json!({"kind":kind,"start":span.start,"end":span.end}))
                .collect::<Vec<_>>()
        );
        view["original"] = original;
    }
    Ok(view)
}

fn intersect_known_extent(anchor: &SourceAnchorRequest, total: u32) -> Option<Interval> {
    (total > 0 && anchor.start <= total).then(|| Interval {
        start: anchor.start,
        end: anchor.end.unwrap_or(total).min(total),
    })
}

fn union_intervals(mut spans: Vec<Interval>) -> Vec<Interval> {
    spans.sort_unstable_by_key(|span| (span.start, span.end));
    let mut union: Vec<Interval> = Vec::new();
    for span in spans {
        if let Some(last) = union.last_mut()
            && u64::from(span.start) <= u64::from(last.end) + 1
        {
            last.end = last.end.max(span.end);
        } else {
            union.push(span);
        }
    }
    union
}

fn uncovered(demand: Interval, spans: &[Interval]) -> Vec<Interval> {
    subtract_intervals(&[demand], spans)
}

fn subtract_intervals(demands: &[Interval], spans: &[Interval]) -> Vec<Interval> {
    let mut gaps = Vec::new();
    let mut index = 0;
    for demand in demands {
        let mut cursor = u64::from(demand.start);
        let end = u64::from(demand.end);
        while index < spans.len() && u64::from(spans[index].end) < cursor {
            index += 1;
        }
        while index < spans.len() && u64::from(spans[index].start) <= end {
            let span = spans[index];
            let start = u64::from(span.start).max(cursor);
            let stop = u64::from(span.end).min(end);
            if start > cursor {
                gaps.push(Interval {
                    start: cursor as u32,
                    end: (start - 1) as u32,
                });
            }
            cursor = cursor.max(stop + 1);
            if u64::from(span.end) <= end {
                index += 1;
            }
            if cursor > end {
                break;
            }
        }
        if cursor <= end {
            gaps.push(Interval {
                start: cursor as u32,
                end: demand.end,
            });
        }
    }
    gaps
}

fn classify_coverage(
    anchor: &SourceAnchorRequest,
    known_extent: Option<u32>,
    facts: &CandidateFacts,
    row: &ManifestRow,
) -> Coverage {
    let demand = match known_extent {
        Some(total) => intersect_known_extent(anchor, total),
        None => {
            let end = anchor.end.unwrap_or(facts.observed_end);
            (end >= anchor.start).then_some(Interval {
                start: anchor.start,
                end,
            })
        }
    };
    let clipped = |readable_only: bool| -> Vec<Interval> {
        union_intervals(
            facts
                .spans
                .iter()
                .filter(|span| !readable_only || span.readable)
                .filter_map(|span| {
                    let demand = demand?;
                    let start = span.interval.start.max(demand.start);
                    let end = span.interval.end.min(demand.end);
                    (start <= end).then_some(Interval { start, end })
                })
                .collect(),
        )
    };
    let present = clipped(false);
    let readable = clipped(true);
    let mut issues = Vec::new();
    if let Some(demand) = demand {
        issues.extend(
            uncovered(demand, &present)
                .into_iter()
                .map(|span| (span, "missing_extraction")),
        );
        issues.extend(
            subtract_intervals(&present, &readable)
                .into_iter()
                .map(|span| (span, "unreadable_extraction")),
        );
    }
    if let (Some(total), Some(end)) = (known_extent, anchor.end)
        && end > total
    {
        issues.push((
            Interval {
                start: anchor.start.max(total.saturating_add(1)),
                end,
            },
            "outside_known_extent",
        ));
    }
    issues.sort_unstable_by_key(|(span, _)| span.start);
    let unknown_tail = known_extent.is_none() && anchor.end.is_none();
    let empty_source = known_extent == Some(0);
    let converter_limited = row.extraction_method.is_some()
        && (row.extraction_coverage.as_deref() != Some("complete")
            || row
                .extraction_missing_json
                .as_deref()
                .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
                .is_some_and(|missing| !missing.is_empty()));
    let limited = row.index_status != "indexed" || converter_limited;
    let status = if empty_source
        || known_extent.is_some_and(|total| anchor.start > total)
        || row.index_status == "pending"
    {
        "partial"
    } else if readable.is_empty() && (!present.is_empty() || row.index_status == "failed") {
        "text_unavailable"
    } else if demand.is_none() || !issues.is_empty() || unknown_tail || limited {
        "partial"
    } else {
        "complete"
    };
    Coverage {
        status,
        issues,
        unknown_tail,
        empty_source,
    }
}

fn read_bounded_original(
    root: &Path,
    anchor: &SourceAnchorRequest,
    row: &ManifestRow,
    current_hash: &str,
    action: SourceAction,
    known_pdf_extent: Option<u32>,
) -> Result<(Option<u32>, Value), String> {
    let cancellation = OperationCancellation::none();
    cancellation.check().map_err(|error| error.to_string())?;
    let opened = open_current_file(root, &anchor.relative, &cancellation)
        .map_err(|error| error.to_string())?;
    if row.source_kind == "pdf" {
        let file_size = u64::try_from(row.file_size.unwrap_or(-1)).unwrap_or(u64::MAX);
        if opened.metadata.len() != file_size && file_size != u64::MAX {
            return Err(
                "source changed after reconciliation; rerun search before inspection".to_owned(),
            );
        }
        let (observed_hash, size) =
            hash_pdf_reference(opened.file, MAX_PDF_SOURCE_BYTES, &cancellation)?;
        if observed_hash != current_hash {
            return Err(
                "source changed after reconciliation; rerun search before inspection".to_owned(),
            );
        }
        if !action.includes_content()
            || known_pdf_extent.is_some_and(|total| intersect_known_extent(anchor, total).is_none())
        {
            return Ok((None, Value::Null));
        }
        return Ok((
            None,
            json!({
                "encoding": "original_page_reference",
                "physical_page": anchor.start,
                "display_label": display_label("page", anchor.start, Some(anchor.start), None),
                "content_sha256": observed_hash,
                "file_size": size,
                "note": if known_pdf_extent.is_some() { "inspect this physical page in the hashed original file; printed page numbers may differ" } else { "requested physical page target; extraction does not establish whether this page exists; printed page numbers may differ" },
            }),
        ));
    }
    let bytes = read_bounded_bytes(
        opened.file,
        &opened.path,
        SOURCES_TEXT_LIMIT_BYTES as usize,
        &cancellation,
    )
    .map_err(|error| error.to_string())?;
    if hex_sha256(&bytes) != current_hash {
        return Err(
            "source changed after reconciliation; rerun search before inspection".to_owned(),
        );
    }
    cancellation.check().map_err(|error| error.to_string())?;
    let text = String::from_utf8(bytes)
        .map_err(|_| "source is not valid UTF-8 text; no text original is served".to_owned())?;
    let total = text.lines().count() as u32;
    if !action.includes_content() {
        return Ok((Some(total), Value::Null));
    }
    let Some(span) = intersect_known_extent(anchor, total) else {
        return Ok((Some(total), Value::Null));
    };
    let selected = text
        .lines()
        .skip(span.start as usize - 1)
        .take((span.end - span.start + 1) as usize)
        .collect::<Vec<_>>()
        .join("\n");
    Ok((
        Some(total),
        json!({
            "encoding": "utf8", "line_start": span.start, "line_end": span.end, "total_lines": total,
            "content_sha256": current_hash, "text": truncate_bytes(&selected, SOURCES_ORIGINAL_BYTE_CAP),
            "truncated": selected.len() > SOURCES_ORIGINAL_BYTE_CAP,
        }),
    ))
}

fn hash_pdf_reference(
    mut reader: impl Read,
    limit: usize,
    cancellation: &OperationCancellation,
) -> Result<(String, usize), String> {
    let mut hash = Sha256::new();
    let mut total = 0usize;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        #[cfg(test)]
        crate::request::test_checkpoint("sources_current_read_chunk");
        cancellation.check().map_err(|error| error.to_string())?;
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read)
            .ok_or_else(|| "source size overflow".to_owned())?;
        if total > limit {
            return Err(format!("input exceeds the {limit}-byte limit"));
        }
        hash.update(&buffer[..read]);
    }
    cancellation.check().map_err(|error| error.to_string())?;
    Ok((hex_digest(&hash.finalize()), total))
}

pub(crate) fn read_bounded_bytes(
    mut file: std::fs::File,
    path: &Path,
    limit: usize,
    cancellation: &OperationCancellation,
) -> Result<Vec<u8>, ManifestError> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        #[cfg(test)]
        crate::request::test_checkpoint("sources_current_read_chunk");
        cancellation.check()?;
        let read = file.read(&mut buffer).map_err(|source| ManifestError::Io {
            action: "read current source",
            path: path.to_path_buf(),
            source,
        })?;
        if read == 0 {
            break;
        }
        if read > limit.saturating_sub(bytes.len()) {
            return Err(ManifestError::Invalid(format!(
                "input exceeds the {limit}-byte limit"
            )));
        }
        bytes.extend_from_slice(&buffer[..read]);
    }
    cancellation.check()?;
    Ok(bytes)
}

fn hex_sha256(bytes: &[u8]) -> String {
    hex_digest(&Sha256::digest(bytes))
}

fn hex_digest(digest: &[u8]) -> String {
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

#[derive(Debug)]
pub(crate) enum PdfCopyError {
    Unavailable(&'static str),
    Source(ManifestError),
}

pub(crate) fn read_authorized_pdf(
    root: &Path,
    path: &str,
    content_sha256: &str,
    cancellation: &OperationCancellation,
) -> Result<Vec<u8>, PdfCopyError> {
    if !is_lowercase_sha256(content_sha256) {
        return Err(PdfCopyError::Unavailable("invalid_content_sha256"));
    }
    let relative = validated_relative_path(path)
        .map_err(|_| PdfCopyError::Unavailable("invalid_source_path"))?;
    cancellation.check().map_err(PdfCopyError::Source)?;
    if crate::retrieval::source_relative_path_is_excluded(root, &relative)
        .map_err(|error| PdfCopyError::Source(error.into()))?
    {
        return Err(PdfCopyError::Unavailable("source_excluded"));
    }
    let prepared =
        prepare_explicit_source(root, &relative, FactsInterest::IdentityOnly, cancellation)
            .map_err(PdfCopyError::Source)?;
    let deferred_reason = match prepared.preparation {
        crate::retrieval::PreparationDisposition::DeferredBudget => {
            Some("source_preparation_deferred_budget")
        }
        crate::retrieval::PreparationDisposition::DeferredConcurrentUpdate => {
            Some("source_preparation_deferred_concurrent_update")
        }
        crate::retrieval::PreparationDisposition::Complete
        | crate::retrieval::PreparationDisposition::PdfContentFailure => None,
    };
    let row = prepared.row.ok_or(PdfCopyError::Unavailable(
        deferred_reason.unwrap_or("source_not_indexed"),
    ))?;
    if !row_present(&row) || row_excluded(&row) {
        return Err(PdfCopyError::Unavailable("pdf_source_not_authorized"));
    }
    if row.source_kind != "pdf" {
        return Err(PdfCopyError::Unavailable("source_not_pdf"));
    }
    if row.content_sha256.as_deref() != Some(content_sha256) {
        return Err(PdfCopyError::Unavailable("source_changed"));
    }
    if row.index_status == "pending"
        && let Some(reason) = deferred_reason
    {
        return Err(PdfCopyError::Unavailable(reason));
    }
    if row.index_status != "indexed" {
        return Err(PdfCopyError::Unavailable("pdf_source_not_authorized"));
    }
    let bytes = verified_current_bytes(
        root,
        &relative,
        content_sha256,
        MAX_PDF_SOURCE_BYTES,
        cancellation,
    )
    .map_err(|error| match error {
        CurrentBytesError::Changed => PdfCopyError::Unavailable("source_changed"),
        CurrentBytesError::Source(error) => PdfCopyError::Source(error),
    })?;
    if crate::retrieval::source_relative_path_is_excluded(root, &relative)
        .map_err(|error| PdfCopyError::Source(error.into()))?
    {
        return Err(PdfCopyError::Unavailable("source_excluded"));
    }
    cancellation.check().map_err(PdfCopyError::Source)?;
    Ok(bytes)
}

#[derive(Debug)]
enum CurrentBytesError {
    Changed,
    Source(ManifestError),
}

fn open_current_file(
    root: &Path,
    relative: &Path,
    cancellation: &OperationCancellation,
) -> Result<crate::retrieval::OpenSource, ManifestError> {
    cancellation.check()?;
    #[cfg(test)]
    crate::request::test_checkpoint("sources_before_current_open");
    cancellation.check()?;
    let mut after_open = |_: &Path| cancellation.check();
    open_workspace_source(root, relative, &mut after_open)
}

fn verified_current_bytes(
    root: &Path,
    relative: &Path,
    expected_hash: &str,
    limit: usize,
    cancellation: &OperationCancellation,
) -> Result<Vec<u8>, CurrentBytesError> {
    let opened =
        open_current_file(root, relative, cancellation).map_err(CurrentBytesError::Source)?;
    let bytes = read_bounded_bytes(opened.file, &opened.path, limit, cancellation)
        .map_err(CurrentBytesError::Source)?;
    if hex_sha256(&bytes) != expected_hash {
        return Err(CurrentBytesError::Changed);
    }
    Ok(bytes)
}

fn truncate_bytes(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn prepared_anchor_kind(value: &str) -> Option<String> {
    match value {
        "line" | "page" => Some(value.to_owned()),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn initialized_source_environment() -> support::TestEnvironment {
        let environment = support::TestEnvironment::from_fixture("basic").unwrap();
        std::fs::create_dir_all(
            environment
                .workspace
                .join(crate::workspace::CONTROL_DIR)
                .join("index"),
        )
        .unwrap();
        std::fs::write(
            environment
                .workspace
                .join(crate::workspace::CONTROL_DIR)
                .join("config.toml"),
            "schema_version=1\nworkspace_id='source-fixture'\n",
        )
        .unwrap();
        crate::learner::LearnerStore::initialize(
            &environment
                .workspace
                .join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite"),
            "source-fixture",
        )
        .unwrap();
        environment
    }

    fn exact_inspection(root: &Path, request: &SourcesRequest) -> Value {
        for _ in 0..3 {
            let value = handle_sources(root, request).unwrap();
            match value["preparation"].as_str() {
                Some("deferred_budget" | "deferred_concurrent_update") => continue,
                _ => return value,
            }
        }
        panic!("exact inspection remained deferred after three bounded calls");
    }

    #[test]
    #[cfg(unix)]
    fn direct_inspection_and_pdf_copy_do_zero_lexical_work_with_positive_search_control() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let environment = initialized_source_environment();
        let root = &environment.workspace;
        let text = b"meaningful lexical positive control\n";
        let pdf = include_bytes!("../../tests/fixtures/interaction/statistics/source-pages.pdf");
        std::fs::write(root.join("lesson\"draft.txt"), text).unwrap();
        std::fs::write(root.join("lesson*draft.pdf"), pdf).unwrap();
        let compiled = Arc::new(AtomicUsize::new(0));
        let ranked = Arc::new(AtomicUsize::new(0));
        let compiled_hook = compiled.clone();
        let ranked_hook = ranked.clone();
        let token = OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
            if checkpoint == "source_query_compilation" {
                compiled_hook.fetch_add(1, Ordering::Relaxed);
            }
            if checkpoint == "source_query_ranking" {
                ranked_hook.fetch_add(1, Ordering::Relaxed);
            }
        }));
        let _scope = crate::request::enter_request_cancellation(&token);
        for (path, bytes, kind) in [
            ("lesson\"draft.txt", text.as_slice(), "line"),
            ("lesson*draft.pdf", pdf.as_slice(), "page"),
        ] {
            let request: SourcesRequest = serde_json::from_value(json!({
                "action":"inspect", "path":path, "content_sha256":hex_sha256(bytes),
                "anchor_kind":kind, "anchor_start":1, "anchor_end":1,
            }))
            .unwrap();
            let value = exact_inspection(root, &request);
            assert_eq!(value["coverage"]["status"], "complete", "{value}");
            assert_eq!(value["file"]["content_sha256"], hex_sha256(bytes));
            assert_eq!(value["original"]["content_sha256"], hex_sha256(bytes));
            if kind == "line" {
                assert_eq!(
                    value["original"]["text"],
                    "meaningful lexical positive control"
                );
            } else {
                assert_eq!(value["original"]["encoding"], "original_page_reference");
            }
        }
        assert_eq!(
            read_authorized_pdf(root, "lesson*draft.pdf", &hex_sha256(pdf), &token).unwrap(),
            pdf
        );
        assert_eq!(
            [
                compiled.load(Ordering::Relaxed),
                ranked.load(Ordering::Relaxed)
            ],
            [0, 0]
        );
        let report = retrieve_workspace_sources(
            root,
            &root
                .join(crate::workspace::CONTROL_DIR)
                .join("index/source-search.sqlite"),
            Path::new(""),
            &RetrievalRequest {
                active_workspace_id: "source-fixture".into(),
                lexical: SourceSearchRequest {
                    query: "positive".into(),
                    path_prefix: None,
                    limit: 20,
                },
            },
            CoordinatorBudget {
                max_duration: std::time::Duration::from_secs(10),
                ..CoordinatorBudget::default()
            },
        )
        .unwrap();
        assert!(
            report
                .hits
                .iter()
                .any(|hit| hit.candidate.path == "lesson\"draft.txt"
                    && hit.candidate.body == "meaningful lexical positive control\n")
        );
        assert!(compiled.load(Ordering::Relaxed) > 0);
        assert_eq!(ranked.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn same_size_change_after_snapshot_is_rejected_before_original_content() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let environment = initialized_source_environment();
        let root = &environment.workspace;
        let original = b"original source\n";
        let changed = b"modified source\n";
        assert_eq!(original.len(), changed.len());
        let path = root.join("lesson.txt");
        std::fs::write(&path, original).unwrap();
        let request: SourcesRequest = serde_json::from_value(
            json!({"action":"inspect", "path":"lesson.txt", "content_sha256":hex_sha256(original)}),
        )
        .unwrap();
        assert_eq!(
            exact_inspection(root, &request)["original"]["text"],
            "original source"
        );
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let swapped = Arc::new(AtomicBool::new(false));
        let swapped_hook = swapped.clone();
        let token = OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
            if checkpoint == "sources_before_current_open"
                && !swapped_hook.swap(true, Ordering::Relaxed)
            {
                std::fs::write(&path, changed).unwrap();
                std::fs::File::options()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_times(std::fs::FileTimes::new().set_modified(modified))
                    .unwrap();
            }
        }));
        let _scope = crate::request::enter_request_cancellation(&token);
        let result = handle_sources(root, &request);
        assert!(swapped.load(Ordering::Relaxed));
        assert!(
            result
                .unwrap_err()
                .contains("source changed after reconciliation")
        );
    }

    #[test]
    fn cancellation_during_current_read_and_pdf_copy_returns_typed_failure() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let environment = initialized_source_environment();
        let root = &environment.workspace;
        let pdf = include_bytes!("../../tests/fixtures/interaction/statistics/source-pages.pdf");
        std::fs::write(root.join("source.pdf"), pdf).unwrap();
        let hash = hex_sha256(pdf);
        let index = root
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite");
        retrieve_workspace_sources(
            root,
            &index,
            Path::new(""),
            &RetrievalRequest {
                active_workspace_id: "source-fixture".into(),
                lexical: SourceSearchRequest {
                    query: "source.pdf".into(),
                    path_prefix: Some(PathBuf::from("source.pdf")),
                    limit: 1,
                },
            },
            CoordinatorBudget {
                max_duration: std::time::Duration::from_secs(10),
                max_on_demand: 1,
                ..CoordinatorBudget::default()
            },
        )
        .unwrap();
        let row = read_manifest_row(&index, Path::new("source.pdf"))
            .unwrap()
            .unwrap();
        assert!(row.present);
        assert_eq!(row.index_status, "indexed");
        assert_eq!(row.content_sha256.as_deref(), Some(hash.as_str()));
        let request: SourcesRequest =
            serde_json::from_value(json!({"action":"inspect", "path":"source.pdf"})).unwrap();
        let inspection = handle_sources(root, &request).unwrap();
        assert_eq!(inspection["coverage"]["status"], "complete");
        assert_eq!(inspection["original"]["content_sha256"], hash);
        let flag = Arc::new(AtomicBool::new(false));
        let flag_hook = flag.clone();
        let token = OperationCancellation::with_flag(flag.clone()).with_test_hook(Arc::new(
            move |checkpoint| {
                if checkpoint == "sources_current_read_chunk" {
                    flag_hook.store(true, Ordering::Relaxed);
                }
            },
        ));
        let _scope = crate::request::enter_request_cancellation(&token);
        assert!(matches!(
            read_authorized_pdf(root, "source.pdf", &hash, &token),
            Err(PdfCopyError::Source(ManifestError::Cancelled))
        ));
        assert!(flag.load(Ordering::Relaxed));
        flag.store(false, Ordering::Relaxed);
        assert!(
            handle_sources(root, &request)
                .unwrap_err()
                .contains("source_operation_cancelled")
        );
        assert!(flag.load(Ordering::Relaxed));
    }

    fn line_anchor(start: u32, end: u32) -> SourceAnchorRequest {
        SourceAnchorRequest {
            path: "lesson.md".to_owned(),
            relative: PathBuf::from("lesson.md"),
            expected_hash: None,
            kind: Some("line".to_owned()),
            start,
            end: Some(end),
        }
    }

    fn facts(spans: &[(u32, u32, bool)]) -> CandidateFacts {
        CandidateFacts {
            observed_end: spans.iter().map(|(_, end, _)| *end).max().unwrap_or(0),
            spans: spans
                .iter()
                .map(|(start, end, readable)| SpanFact {
                    interval: Interval {
                        start: *start,
                        end: *end,
                    },
                    readable: *readable,
                })
                .collect(),
            ..CandidateFacts::default()
        }
    }

    #[test]
    fn interval_policy_distinguishes_missing_blank_and_nonexistent_ranges() {
        let row = pdf_row(&"a".repeat(64), 512);
        let mut anchor = line_anchor(1, 10);
        let coverage = classify_coverage(&anchor, Some(2), &facts(&[(1, 2, true)]), &row);
        assert_eq!(coverage.status, "partial");
        assert_eq!(
            coverage.issues,
            vec![(Interval { start: 3, end: 10 }, "outside_known_extent")]
        );
        anchor.start = 10;
        anchor.end = Some(12);
        assert_eq!(intersect_known_extent(&anchor, 2), None);
        let coverage = classify_coverage(&anchor, Some(2), &facts(&[(1, 2, true)]), &row);
        assert_eq!(coverage.status, "partial");
        assert_eq!(
            coverage.issues,
            vec![(Interval { start: 10, end: 12 }, "outside_known_extent")]
        );
        anchor.start = 1;
        anchor.end = Some(3);
        let missing = classify_coverage(&anchor, None, &facts(&[(1, 1, true), (3, 3, true)]), &row);
        assert_eq!(missing.status, "partial");
        assert_eq!(
            missing.issues,
            vec![(Interval { start: 2, end: 2 }, "missing_extraction")]
        );
        let blank = classify_coverage(
            &anchor,
            Some(3),
            &facts(&[(1, 1, true), (2, 2, false), (3, 3, true)]),
            &row,
        );
        assert_eq!(
            blank.issues,
            vec![(Interval { start: 2, end: 2 }, "unreadable_extraction")]
        );
        let overlap = classify_coverage(
            &anchor,
            Some(3),
            &facts(&[(1, 2, true), (2, 2, false), (2, 3, true)]),
            &row,
        );
        assert_eq!(overlap.status, "complete");
        assert_eq!(overlap.issues, vec![]);
    }

    #[test]
    fn explicit_maximum_is_finite_and_union_work_is_interval_bounded() {
        let row = pdf_row(&"a".repeat(64), 512);
        let anchor = line_anchor(1, u32::MAX);
        let coverage = classify_coverage(
            &anchor,
            None,
            &facts(&[(1, 2, true), (u32::MAX, u32::MAX, true)]),
            &row,
        );
        assert_eq!(coverage.status, "partial");
        assert_eq!(
            coverage.issues,
            vec![(
                Interval {
                    start: 3,
                    end: u32::MAX - 1
                },
                "missing_extraction"
            )]
        );
        assert!(!coverage.unknown_tail);
        let covered = classify_coverage(
            &anchor,
            None,
            &facts(&[(1, u32::MAX - 1, true), (u32::MAX, u32::MAX, true)]),
            &row,
        );
        assert_eq!(covered.status, "complete");
        assert_eq!(covered.issues, vec![]);
    }

    #[test]
    fn sparse_readability_subtraction_preserves_every_unreadable_span() {
        let present = (0..10_000)
            .map(|index| Interval {
                start: index * 3 + 1,
                end: index * 3 + 2,
            })
            .collect::<Vec<_>>();
        let readable = (0..10_000)
            .map(|index| Interval {
                start: index * 3 + 1,
                end: index * 3 + 1,
            })
            .collect::<Vec<_>>();
        let missing = subtract_intervals(&present, &readable);
        assert_eq!(missing.len(), 10_000);
        assert_eq!(missing[0], Interval { start: 2, end: 2 });
        assert_eq!(
            missing[9_999],
            Interval {
                start: 29_999,
                end: 29_999
            }
        );
        assert_eq!(
            subtract_intervals(
                &present,
                &[Interval {
                    start: 1,
                    end: 30_000
                }]
            ),
            vec![]
        );
    }

    #[test]
    fn omitted_end_empty_source_and_converter_limits_remain_distinct() {
        let mut anchor = line_anchor(1, 2);
        anchor.end = None;
        let mut row = worker_row();
        row.extraction_coverage = Some("complete".to_owned());
        row.extraction_missing_json = Some("[]".to_owned());
        let observed = facts(&[(1, 2, true)]);
        let unknown = classify_coverage(&anchor, None, &observed, &row);
        assert_eq!(unknown.status, "partial");
        assert!(unknown.unknown_tail);
        assert_eq!(unknown.issues, vec![]);
        let known = classify_coverage(&anchor, Some(2), &observed, &row);
        assert_eq!(known.status, "complete");
        assert!(!known.unknown_tail);
        let empty = classify_coverage(&anchor, Some(0), &facts(&[(1, 1, false)]), &row);
        assert_eq!(empty.status, "partial");
        assert!(empty.empty_source);
        assert_eq!(empty.issues, vec![]);
        anchor.end = Some(2);
        row.extraction_coverage = Some("partial".to_owned());
        let limited = classify_coverage(&anchor, None, &observed, &row);
        assert_eq!(limited.status, "partial");
        row.index_status = "failed".to_owned();
        let unavailable = classify_coverage(&anchor, None, &facts(&[(1, 2, false)]), &row);
        assert_eq!(unavailable.status, "text_unavailable");
        row.index_status = "pending".to_owned();
        assert_eq!(
            classify_coverage(&anchor, None, &observed, &row).status,
            "partial"
        );
    }

    #[test]
    fn pending_pdf_inspection_keeps_empty_text_and_exact_hashed_physical_page() {
        use std::fs;

        let environment = support::TestEnvironment::from_fixture("basic").unwrap();
        let pdf = include_bytes!("../../tests/fixtures/interaction/statistics/source-pages.pdf");
        fs::write(environment.workspace.join("math.pdf"), pdf).unwrap();
        let hash = hex_sha256(pdf);
        let root = &environment.workspace;
        let index = root
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite");
        let workspace_id = "source-fixture".to_owned();
        fs::create_dir_all(index.parent().unwrap()).unwrap();
        fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"source-fixture\"\n",
        )
        .unwrap();
        // The real coordinator discovers/hashes the PDF without starting
        // extraction. This deterministically creates the state returned by a
        // cold preparation that exhausted its budget; no sleeps or SQL writes.
        retrieve_workspace_sources(
            root,
            &index,
            Path::new(""),
            &RetrievalRequest {
                active_workspace_id: workspace_id.clone(),
                lexical: SourceSearchRequest {
                    query: "math.pdf".into(),
                    path_prefix: Some(PathBuf::from("math.pdf")),
                    limit: 50,
                },
            },
            CoordinatorBudget {
                max_on_demand: 0,
                max_duration: std::time::Duration::from_secs(5),
                ..CoordinatorBudget::default()
            },
        )
        .unwrap();
        let row = read_manifest_row(&index, Path::new("math.pdf"))
            .unwrap()
            .unwrap();
        assert_eq!(row.index_status, "pending");
        assert!(row.last_error.is_none());
        let anchor = SourceAnchorRequest {
            path: "math.pdf".into(),
            relative: PathBuf::from("math.pdf"),
            expected_hash: Some(hash.clone()),
            kind: Some("page".into()),
            start: 2,
            end: Some(2),
        };
        let inspected = open_current_source(
            SourceAction::Inspect,
            &workspace_id,
            root,
            &anchor,
            &row,
            load_current_candidates(
                &Connection::open(&index).unwrap(),
                &index,
                &row,
                anchor.start,
                anchor.end,
                true,
                &OperationCancellation::none(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(inspected["coverage"]["status"], "partial");
        assert_eq!(inspected["coverage"]["missing_ranges"], json!(["page 2-2"]));
        assert!(inspected["coverage"]["last_error"].is_null());
        assert_eq!(inspected["excerpt"], "");
        assert_eq!(inspected["file"]["content_sha256"], hash);
        assert_eq!(inspected["original"]["content_sha256"], hash);
        assert_eq!(inspected["original"]["physical_page"], 2);
        assert_eq!(inspected["anchor"]["physical_start"], 2);
        assert_eq!(inspected["anchor"]["physical_end"], 2);
        assert_eq!(inspected["basis"]["visual_analysis"], false);

        // A successful unbound source-open can describe this pending PDF and
        // offer its original-page reference. That is not an indexed byte-read
        // authorization. Hold the reconciliation writer without changing any
        // rows so the next real requests deterministically stay pending: they
        // defer within their budget instead of waiting for the writer.
        crate::learner::LearnerStore::initialize(
            &root
                .join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite"),
            &workspace_id,
        )
        .unwrap();
        let writer = Connection::open(&index).unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        let (_, resolved) = resolve_workspace(root).unwrap();
        let resolved = resolved.unwrap();
        let reference = SourcesRequest {
            action: SourceAction::Inspect,
            schema_version: None,
            path: Some("math.pdf".into()),
            content_sha256: None,
            anchor_kind: Some("page".into()),
            anchor_start: Some(2),
            anchor_end: Some(2),
        };
        let deferred = sources_resolved(&resolved, SourceAction::Inspect, &reference).unwrap();
        assert_eq!(deferred["preparation"], "deferred_budget", "{deferred}");
        assert_eq!(deferred["excerpt"], "", "{deferred}");
        assert_eq!(deferred["coverage"]["status"], "partial", "{deferred}");
        match read_authorized_pdf(root, "math.pdf", &hash, &OperationCancellation::none()) {
            Err(PdfCopyError::Unavailable("source_preparation_deferred_budget")) => {}
            other => panic!("a deferred pending PDF must not be copied: {other:?}"),
        }
        assert_eq!(
            read_manifest_row(&index, Path::new("math.pdf"))
                .unwrap()
                .unwrap()
                .index_status,
            "pending"
        );
        writer.execute_batch("ROLLBACK").unwrap();
    }

    #[test]
    fn display_labels_name_physical_indices_not_printed_numbers() {
        assert_eq!(display_label("page", 3, Some(3), None), "physical page 3");
        assert_eq!(
            display_label("page", 2, Some(5), None),
            "physical pages 2-5"
        );
        assert_eq!(display_label("line", 7, Some(7), None), "line 7");
        assert_eq!(display_label("line", 1, None, None), "lines 1 to end");
        assert_eq!(
            display_label("page", 1, None, None),
            "physical pages 1 to end"
        );
        assert_eq!(
            display_label("line", 7, Some(9), Some("## WAL")),
            "lines 7-9 (## WAL)"
        );
        let anchor = anchor_view(&line_anchor(7, 9), Some("## WAL"));
        assert!(
            anchor["page_numbering"]
                .as_str()
                .unwrap()
                .contains("physical")
        );
    }

    #[test]
    fn external_citations_stay_distinguishable_with_snapshot_guidance() {
        let view = external_citation(SourceAction::Inspect, "https://example.com/paper.pdf");
        assert_eq!(view["authority"]["type"], "external_citation");
        assert!(view.get("original").is_none());
        assert!(
            view["snapshot_guidance"]
                .as_str()
                .unwrap()
                .contains("explicitly authorize")
        );
    }

    #[test]
    fn traversal_and_absolute_paths_are_rejected_before_any_read() {
        for bad in [
            "../lesson.md",
            "/etc/hostname",
            "a/../../b",
            "lesson.md\0",
            "",
        ] {
            assert!(
                validated_relative_path(bad).is_err(),
                "path must be rejected: {bad:?}"
            );
        }
        assert!(validated_relative_path("notes/lesson.md").is_ok());
    }

    fn pdf_row(hash: &str, size: u64) -> ManifestRow {
        ManifestRow {
            manifest_id: 1,
            source_revision: 1,
            display_path: "doc.pdf".to_owned(),
            source_kind: "pdf".to_owned(),
            content_sha256: Some(hash.to_owned()),
            index_status: "indexed".to_owned(),
            file_size: Some(size as i64),
            exclusion_reason: None,
            last_error: None,
            present: true,
            extraction_method: None,
            extraction_method_version: None,
            extraction_coverage: None,
            extraction_missing_json: None,
            extraction_warnings_json: None,
        }
    }

    fn worker_row() -> ManifestRow {
        let mut row = pdf_row(&"a".repeat(64), 512);
        row.extraction_method = Some("docling".to_owned());
        row.extraction_method_version = Some("docling 2.126.0".to_owned());
        row.extraction_coverage = Some("partial".to_owned());
        row.extraction_missing_json = Some("[\"remainder beyond worker bounds\"]".to_owned());
        row.extraction_warnings_json = Some(
            "[\"uncertain table structure; verify against the original page view\"]".to_owned(),
        );
        row
    }

    #[test]
    fn converter_provenance_surfaces_method_and_warnings() {
        let native = extraction_view("pdf", Some(&pdf_row(&"a".repeat(64), 512)));
        assert_eq!(native["method"], "native-pdf-worker/v1");
        assert!(native["converter"].is_null());
        let derived = extraction_view("pdf", Some(&worker_row()));
        assert_eq!(derived["method"], "docling");
        assert_eq!(derived["version"], "docling 2.126.0");
        assert_eq!(derived["converter"]["coverage"], "partial");
        assert_eq!(
            derived["converter"]["missing_ranges"],
            serde_json::json!(["remainder beyond worker bounds"])
        );
        assert!(
            derived["converter"]["warnings"]
                .as_array()
                .is_some_and(|warnings| warnings.len() == 1),
            "{derived}"
        );
        assert!(extraction_view("unknown", None)["method"].is_string());
    }

    fn page_anchor() -> SourceAnchorRequest {
        SourceAnchorRequest {
            path: "doc.pdf".to_owned(),
            relative: PathBuf::from("doc.pdf"),
            expected_hash: None,
            kind: Some("page".to_owned()),
            start: 1,
            end: Some(1),
        }
    }

    /// A same-size replacement must never receive the previous file's
    /// hash: the PDF branch hashes the opened bytes exactly like the
    /// text branch instead of trusting manifest memory.
    #[test]
    fn pdf_inspection_hashes_opened_bytes_not_manifest_memory() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "magnesium-sources-pdf-hash-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).expect("fixture root");
        // Two revisions with identical byte length but different bytes.
        let first = b"%PDF-1.4 revision one payload ............";
        let mut second = first.to_vec();
        second[11] = b't';
        second[12] = b'w';
        assert_eq!(first.len(), second.len());
        assert_ne!(first, second.as_slice());
        std::fs::write(root.join("doc.pdf"), first).expect("write revision one");
        let hash_v1 = hex_sha256(first);
        let row = pdf_row(&hash_v1, first.len() as u64);
        let anchor = page_anchor();
        let inspected = read_bounded_original(
            &root,
            &anchor,
            &row,
            &hash_v1,
            SourceAction::Inspect,
            Some(1),
        )
        .expect("current revision inspects");
        assert_eq!(inspected.1["content_sha256"], hash_v1);
        assert_eq!(inspected.1["encoding"], "original_page_reference");
        std::fs::write(root.join("doc.pdf"), &second).expect("write revision two");
        let stale = read_bounded_original(
            &root,
            &anchor,
            &row,
            &hash_v1,
            SourceAction::Inspect,
            Some(1),
        )
        .expect_err("same-size replacement must not receive the old hash");
        assert!(stale.contains("changed"), "{stale}");
        std::fs::remove_dir_all(&root).expect("fixture cleanup");
    }
    #[test]
    fn pdf_reference_hashing_is_chunk_bounded_and_cancellable_at_reads_and_eof() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        struct Probe {
            bytes: std::io::Cursor<Vec<u8>>,
            flag: Arc<AtomicBool>,
            cancel_after_data: bool,
            cancel_at_eof: bool,
            largest_request: usize,
            reads: usize,
        }
        impl Read for Probe {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.largest_request = self.largest_request.max(buffer.len());
                self.reads += 1;
                let count = self.bytes.read(buffer)?;
                if (self.cancel_after_data && count > 0) || (self.cancel_at_eof && count == 0) {
                    self.flag.store(true, Ordering::Relaxed);
                }
                Ok(count)
            }
        }
        let bytes = vec![b'x'; 3 * 64 * 1024 + 7];
        let flag = Arc::new(AtomicBool::new(false));
        let mut probe = Probe {
            bytes: std::io::Cursor::new(bytes.clone()),
            flag: flag.clone(),
            cancel_after_data: false,
            cancel_at_eof: false,
            largest_request: 0,
            reads: 0,
        };
        let token = OperationCancellation::with_flag(flag.clone());
        let (hash, size) = hash_pdf_reference(&mut probe, bytes.len(), &token).unwrap();
        assert_eq!(hash, hex_sha256(&bytes));
        assert_eq!(size, bytes.len());
        assert_eq!(probe.largest_request, 64 * 1024);
        assert_eq!(probe.reads, 5);
        probe.bytes.set_position(0);
        assert_eq!(
            hash_pdf_reference(&mut probe, bytes.len() - 1, &token).unwrap_err(),
            format!("input exceeds the {}-byte limit", bytes.len() - 1)
        );
        for at_eof in [false, true] {
            flag.store(false, Ordering::Relaxed);
            probe.bytes.set_position(0);
            probe.reads = 0;
            probe.cancel_after_data = !at_eof;
            probe.cancel_at_eof = at_eof;
            let error = hash_pdf_reference(&mut probe, bytes.len(), &token).unwrap_err();
            assert!(error.contains("cancelled"), "{error}");
            assert_eq!(probe.reads, if at_eof { 5 } else { 1 });
        }
    }

    #[test]
    fn pdf_status_and_outside_ranges_verify_bytes_after_manifest_snapshot() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let pdf = include_bytes!("../../tests/fixtures/interaction/statistics/source-pages.pdf");
        let hash = hex_sha256(pdf);
        let mut replacement = pdf.to_vec();
        replacement[7] = if replacement[7] == b'7' { b'6' } else { b'7' };
        assert_eq!(replacement.len(), pdf.len());
        assert_ne!(hex_sha256(&replacement), hash);
        for action in [SourceAction::Status, SourceAction::Inspect] {
            for bound_hash in [false, true] {
                for deleted in [false, true] {
                    let environment = support::TestEnvironment::from_fixture("basic").unwrap();
                    let root = &environment.workspace;
                    let index = root
                        .join(crate::workspace::CONTROL_DIR)
                        .join("index/source-search.sqlite");
                    let target = root.join("source.pdf");
                    std::fs::create_dir_all(index.parent().unwrap()).unwrap();
                    std::fs::write(&target, pdf).unwrap();
                    std::fs::write(
                        root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
                        "schema_version = 1\nworkspace_id = \"source-freshness\"\n",
                    )
                    .unwrap();
                    crate::learner::LearnerStore::initialize(
                        &root
                            .join(crate::workspace::CONTROL_DIR)
                            .join("state.sqlite"),
                        "source-freshness",
                    )
                    .unwrap();
                    let request = RetrievalRequest {
                        active_workspace_id: "source-freshness".to_owned(),
                        lexical: SourceSearchRequest {
                            query: "source.pdf".to_owned(),
                            path_prefix: Some(PathBuf::from("source.pdf")),
                            limit: 50,
                        },
                    };
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    let mut prepared = false;
                    for _ in 0..32 {
                        let remaining =
                            deadline.saturating_duration_since(std::time::Instant::now());
                        assert!(
                            !remaining.is_zero(),
                            "PDF fixture preparation deadline exhausted"
                        );
                        let report = retrieve_workspace_sources(
                            root,
                            &index,
                            Path::new(""),
                            &request,
                            CoordinatorBudget {
                                max_duration: remaining,
                                ..CoordinatorBudget::default()
                            },
                        )
                        .unwrap();
                        assert!(
                            std::time::Instant::now() < deadline,
                            "PDF fixture preparation exceeded its shared deadline"
                        );
                        for hit in &report.hits {
                            assert_eq!(hit.candidate.path, "source.pdf");
                            assert_eq!(hit.candidate.content_sha256, hash);
                        }
                        let Some(row) = read_manifest_row(&index, Path::new("source.pdf")).unwrap()
                        else {
                            assert!(
                                report.reconciliation.deferred(),
                                "undiscovered PDF must still be preparing"
                            );
                            continue;
                        };
                        assert!(row.present);
                        assert_eq!(row.source_kind, "pdf");
                        assert_eq!(row.content_sha256.as_deref(), Some(hash.as_str()));
                        assert!(row.exclusion_reason.is_none());
                        if row.index_status == "indexed" {
                            prepared = true;
                            break;
                        }
                        assert_eq!(
                            row.index_status, "pending",
                            "terminal PDF fixture preparation state"
                        );
                        assert!(report.hits.is_empty());
                    }
                    assert!(prepared, "PDF fixture preparation request cap exhausted");
                    let (_, resolved) = resolve_workspace(root).unwrap();
                    let resolved = resolved.unwrap();
                    let reference = SourcesRequest {
                        action: SourceAction::Inspect,
                        schema_version: None,
                        path: Some("source.pdf".to_owned()),
                        content_sha256: bound_hash.then(|| hash.clone()),
                        anchor_kind: None,
                        anchor_start: (action.includes_content()).then_some(u32::MAX),
                        anchor_end: (action.includes_content()).then_some(u32::MAX),
                    };
                    let current = sources_resolved(&resolved, action, &reference).unwrap();
                    assert_eq!(current["file"]["content_sha256"], hash);
                    if !action.includes_content() {
                        assert_eq!(current["coverage"]["status"], "complete");
                        assert!(current.get("excerpt").is_none());
                        assert!(current.get("original").is_none());
                    } else {
                        assert_eq!(current["coverage"]["status"], "partial");
                        assert!(current["original"].is_null());
                        assert_eq!(
                            current["coverage"]["range_issues"][0]["reason"],
                            "outside_known_extent"
                        );
                    }
                    let changed = Arc::new(AtomicBool::new(false));
                    let changed_hook = changed.clone();
                    let changed_target = target.clone();
                    let replacement_hook = replacement.clone();
                    let token =
                        OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
                            if checkpoint == "sources_manifest_read"
                                && !changed_hook.swap(true, Ordering::Relaxed)
                            {
                                if deleted {
                                    std::fs::remove_file(&changed_target).unwrap();
                                } else {
                                    std::fs::write(&changed_target, &replacement_hook).unwrap();
                                }
                            }
                        }));
                    let error = {
                        let _scope = crate::request::enter_request_cancellation(&token);
                        sources_resolved(&resolved, action, &reference).unwrap_err()
                    };
                    assert!(changed.load(Ordering::Relaxed));
                    if deleted {
                        assert!(
                            error.contains("inspect source") && error.contains("source.pdf"),
                            "{error}"
                        );
                    } else {
                        assert_eq!(
                            error,
                            "source changed after reconciliation; rerun search before inspection"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn source_reader_pins_manifest_and_candidate_generation_in_one_snapshot() {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        let environment = support::TestEnvironment::from_fixture("basic").unwrap();
        let root = &environment.workspace;
        let index = root
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite");
        let workspace_id = "source-snapshot";
        std::fs::create_dir_all(index.parent().unwrap()).unwrap();
        std::fs::write(root.join("lesson.md"), "old coherent generation\n").unwrap();
        std::fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"source-snapshot\"\n",
        )
        .unwrap();
        crate::learner::LearnerStore::initialize(
            &root
                .join(crate::workspace::CONTROL_DIR)
                .join("state.sqlite"),
            workspace_id,
        )
        .unwrap();
        retrieve_workspace_sources(
            root,
            &index,
            Path::new(""),
            &RetrievalRequest {
                active_workspace_id: workspace_id.to_owned(),
                lexical: SourceSearchRequest {
                    query: "lesson.md".to_owned(),
                    path_prefix: Some(PathBuf::from("lesson.md")),
                    limit: 50,
                },
            },
            CoordinatorBudget::default(),
        )
        .unwrap();
        let (_, resolved) = resolve_workspace(root).unwrap();
        let resolved = resolved.unwrap();
        let published = Arc::new(AtomicBool::new(false));
        let published_hook = published.clone();
        let index_hook = index.clone();
        let token = OperationCancellation::none().with_test_hook(Arc::new(move |checkpoint| {
            if checkpoint == "sources_manifest_read" && !published_hook.swap(true, Ordering::Relaxed) {
                let writer = Connection::open(&index_hook).unwrap();
                writer.execute_batch("BEGIN IMMEDIATE;
                    UPDATE source_manifest SET extraction_method = 'docling', extraction_method_version = 'snapshot-new', extraction_coverage = 'complete', extraction_missing_json = '[]' WHERE display_path = 'lesson.md';
                    UPDATE source_candidates SET body = 'new coherent generation' WHERE status = 'current';
                    COMMIT;").unwrap();
            }
        }));
        let reference = SourcesRequest {
            action: SourceAction::Inspect,
            schema_version: None,
            path: Some("lesson.md".to_owned()),
            content_sha256: None,
            anchor_kind: None,
            anchor_start: Some(1),
            anchor_end: Some(1),
        };
        let old = {
            let _scope = crate::request::enter_request_cancellation(&token);
            sources_resolved(&resolved, SourceAction::Inspect, &reference).unwrap()
        };
        assert!(published.load(Ordering::Relaxed));
        assert_eq!(old["extraction"]["method"], "native-text/v1");
        assert_eq!(old["excerpt"], "old coherent generation\n");
        let new = sources_resolved(&resolved, SourceAction::Inspect, &reference).unwrap();
        assert_eq!(new["extraction"]["method"], "docling");
        assert_eq!(new["extraction"]["version"], "snapshot-new");
        assert_eq!(new["excerpt"], "new coherent generation");
        assert_eq!(old["file"]["content_sha256"], new["file"]["content_sha256"]);
        let connection = Connection::open(&index).unwrap();
        let row = read_manifest_row_from(&connection, &index, Path::new("lesson.md"))
            .unwrap()
            .unwrap();
        connection
            .execute(
                "UPDATE source_candidates SET body = ?1 WHERE status = 'current'",
                ["\u{2003}\u{a0}\u{3000}"],
            )
            .unwrap();
        connection.execute("INSERT INTO source_candidates(candidate_id,manifest_id,content_sha256,anchor_kind,anchor_start,anchor_end,body,status) VALUES (?1,?2,?3,'line',100,100,'unrelated body','current')", rusqlite::params!["b".repeat(64), row.manifest_id, row.content_sha256]).unwrap();
        let anchor = line_anchor(1, 1);
        let status = load_current_candidates(
            &connection,
            &index,
            &row,
            anchor.start,
            anchor.end,
            false,
            &OperationCancellation::none(),
        )
        .unwrap();
        assert_eq!(status.observed_end, 100);
        assert_eq!(status.spans.len(), 1);
        assert!(!status.spans[0].readable);
        assert!(status.excerpt.is_none());
        assert!(status.excerpt_ranges.is_empty());
        assert_eq!(
            classify_coverage(&anchor, Some(1), &status, &row).status,
            "text_unavailable"
        );
    }
}
