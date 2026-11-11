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
use super::sha256_hex;
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

/// Bounded original cap for one `sources` response: it keeps one anchor's raw
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
    let Some(workspace) = resolve_workspace(cwd)? else {
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
    if sha256_hex(&bytes) != current_hash {
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
    if sha256_hex(&bytes) != expected_hash {
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
mod tests;
