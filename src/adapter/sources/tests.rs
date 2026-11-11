use super::*;

use std::fs;

/// A temporary study folder that holds the `basic` fixture's lesson. It
/// is removed when the test ends.
struct TestEnvironment {
    root: PathBuf,
    workspace: PathBuf,
}

impl TestEnvironment {
    fn basic() -> Self {
        let root =
            std::env::temp_dir().join(crate::learner::store::new_id("source-unit-test").unwrap());
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            workspace.join("lesson.md"),
            include_str!("../../../tests/fixtures/workspaces/basic/lesson.md"),
        )
        .unwrap();
        Self { root, workspace }
    }
}

impl Drop for TestEnvironment {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn initialized_source_environment() -> TestEnvironment {
    let environment = TestEnvironment::basic();
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
    let pdf = include_bytes!("../../../tests/fixtures/interaction/statistics/source-pages.pdf");
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
            "action":"inspect", "path":path, "content_sha256":sha256_hex(bytes),
            "anchor_kind":kind, "anchor_start":1, "anchor_end":1,
        }))
        .unwrap();
        let value = exact_inspection(root, &request);
        assert_eq!(value["coverage"]["status"], "complete", "{value}");
        assert_eq!(value["file"]["content_sha256"], sha256_hex(bytes));
        assert_eq!(value["original"]["content_sha256"], sha256_hex(bytes));
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
        read_authorized_pdf(root, "lesson*draft.pdf", &sha256_hex(pdf), &token).unwrap(),
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
        json!({"action":"inspect", "path":"lesson.txt", "content_sha256":sha256_hex(original)}),
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
    let pdf = include_bytes!("../../../tests/fixtures/interaction/statistics/source-pages.pdf");
    std::fs::write(root.join("source.pdf"), pdf).unwrap();
    let hash = sha256_hex(pdf);
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

    let environment = TestEnvironment::basic();
    let pdf = include_bytes!("../../../tests/fixtures/interaction/statistics/source-pages.pdf");
    fs::write(environment.workspace.join("math.pdf"), pdf).unwrap();
    let hash = sha256_hex(pdf);
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
    let resolved = resolve_workspace(root).unwrap();
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
    row.extraction_warnings_json =
        Some("[\"uncertain table structure; verify against the original page view\"]".to_owned());
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
    let hash_v1 = sha256_hex(first);
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
    assert_eq!(hash, sha256_hex(&bytes));
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
    let pdf = include_bytes!("../../../tests/fixtures/interaction/statistics/source-pages.pdf");
    let hash = sha256_hex(pdf);
    let mut replacement = pdf.to_vec();
    replacement[7] = if replacement[7] == b'7' { b'6' } else { b'7' };
    assert_eq!(replacement.len(), pdf.len());
    assert_ne!(sha256_hex(&replacement), hash);
    for action in [SourceAction::Status, SourceAction::Inspect] {
        for bound_hash in [false, true] {
            for deleted in [false, true] {
                let environment = TestEnvironment::basic();
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
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
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
                let resolved = resolve_workspace(root).unwrap();
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
    let environment = TestEnvironment::basic();
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
    let resolved = resolve_workspace(root).unwrap();
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
