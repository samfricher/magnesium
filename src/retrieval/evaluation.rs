//! Test-only quality and operational regression for structural+FTS5 source search.
//!
//! Fixed queries and independent relevance judgments live under
//! `tests/fixtures/retrieval`.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::boundary::{RetrievalRequest, retrieve_workspace_sources};
use super::coordinator::{CoordinatorBudget, IndexCoverage, coordinate_source_query};
use super::manifest::{manifest_coverage, pending_source_paths};
use super::search::{SourceSearchHit, SourceSearchReport, SourceSearchRequest};

const CORPUS_JSON: &str = include_str!("../../tests/fixtures/retrieval/corpus.json");
const POLICY_JSON: &str = include_str!("../../tests/fixtures/retrieval/baseline-policy.json");
const FIXTURE_ROOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/retrieval/workspaces"
);
static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Corpus {
    version: u32,
    workspaces: Vec<WorkspaceJudgments>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceJudgments {
    id: String,
    fixture: String,
    queries: Vec<QueryJudgment>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryJudgment {
    id: String,
    kind: QueryKind,
    query: String,
    relevant_paths: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum QueryKind {
    Exact,
    Paraphrased,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BaselinePolicy {
    version: u32,
    backend: String,
    regression: RegressionThresholds,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegressionThresholds {
    minimum_recall_at_5: f64,
    minimum_recall_at_10: f64,
    minimum_mrr: f64,
    maximum_wrong_workspace_rate: f64,
}

#[derive(Clone, Debug, Serialize)]
struct BenchmarkReport {
    schema_version: u32,
    backend: &'static str,
    corpus_version: u32,
    quality: QualityMetrics,
    operations: OperationalMetrics,
}

#[derive(Clone, Debug, Default, Serialize)]
struct QualityMetrics {
    queries: usize,
    exact_queries: usize,
    paraphrased_queries: usize,
    relevance_judgments: usize,
    recall_at_5: f64,
    recall_at_10: f64,
    mrr: f64,
    wrong_workspace_rate: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
struct OperationalMetrics {
    recovery_preserved_canonical_state: bool,
    external_detection_queries: usize,
    large_repository_max_metadata_per_call: usize,
    large_repository_max_content_reads_per_call: usize,
    partial_index_retrieval_succeeded: bool,
}

#[derive(Default)]
struct QualityAccumulator {
    queries: usize,
    exact_queries: usize,
    paraphrased_queries: usize,
    relevant: usize,
    recalled_at_5: usize,
    recalled_at_10: usize,
    reciprocal_rank: f64,
    workspace_scope_checks: usize,
    wrong_workspace_violations: usize,
}

struct WorkspaceFixture {
    root: PathBuf,
    id: String,
}

impl WorkspaceFixture {
    fn from_checked_in(judgments: &WorkspaceJudgments) -> Self {
        let fixture = Self::empty(&judgments.id);
        copy_tree(
            &Path::new(FIXTURE_ROOT).join(&judgments.fixture),
            &fixture.root,
        );
        fixture.initialize();
        fixture
    }

    fn empty(id: &str) -> Self {
        let sequence = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("benchmark clock after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "magnesium-retrieval-eval-{}-{sequence}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create retrieval evaluation workspace");
        Self {
            root,
            id: id.to_owned(),
        }
    }

    fn initialize(&self) {
        fs::create_dir_all(self.root.join(crate::workspace::CONTROL_DIR).join("index"))
            .expect("create retrieval evaluation index directory");
        fs::write(
            self.root
                .join(crate::workspace::CONTROL_DIR)
                .join("config.toml"),
            format!("workspace_id = \"{}\"\n", self.id),
        )
        .expect("write retrieval evaluation workspace identity");
    }

    fn index(&self) -> PathBuf {
        self.root
            .join(crate::workspace::CONTROL_DIR)
            .join("index/source-search.sqlite")
    }

    fn write(&self, relative: &str, contents: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("evaluation fixture file parent"))
            .expect("create evaluation fixture directory");
        fs::write(path, contents).expect("write evaluation fixture file");
    }
}

impl Drop for WorkspaceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_benchmark(warm_repeats: usize, large_repository_files: usize) -> BenchmarkReport {
    let corpus: Corpus = serde_json::from_str(CORPUS_JSON).expect("parse retrieval corpus");
    let policy: BaselinePolicy =
        serde_json::from_str(POLICY_JSON).expect("parse retrieval baseline policy");
    assert_eq!(corpus.version, 1);
    assert_eq!(policy.version, 1);
    assert_eq!(policy.backend, "structural_fts5");
    validate_corpus(&corpus);

    let mut quality = QualityAccumulator::default();
    let mut recovery_preserved_canonical_state = false;
    let mut external_detection_queries = 0;

    for (workspace_index, judgments) in corpus.workspaces.iter().enumerate() {
        let fixture = WorkspaceFixture::from_checked_in(judgments);
        assert!(judgments.queries.iter().all(|query| {
            query
                .relevant_paths
                .iter()
                .all(|path| fixture.root.join(path).is_file())
        }));
        index_workspace(&fixture);

        for judgment in &judgments.queries {
            let report = retrieve(&fixture, Path::new(""), &judgment.query, budget(256, 16));
            score_query(&mut quality, judgment, &report.hits);
            for _ in 0..warm_repeats {
                let warm = retrieve(&fixture, Path::new(""), &judgment.query, budget(256, 16));
                assert_eq!(hit_identity(&warm.hits), hit_identity(&report.hits));
            }
        }

        if workspace_index == 0 {
            external_detection_queries = measure_external_detection(&fixture);
            recovery_preserved_canonical_state = measure_recovery(&fixture, judgments);
        }
    }
    quality.workspace_scope_checks += 1;
    quality.wrong_workspace_violations += usize::from(wrong_workspace_control_violated());

    let large = measure_large_repository(large_repository_files);
    BenchmarkReport {
        schema_version: 1,
        backend: "structural_fts5",
        corpus_version: corpus.version,
        quality: quality.finish(),
        operations: OperationalMetrics {
            recovery_preserved_canonical_state,
            external_detection_queries,
            large_repository_max_metadata_per_call: large.max_metadata_per_call,
            large_repository_max_content_reads_per_call: large.max_content_reads_per_call,
            partial_index_retrieval_succeeded: large.partial_retrieval_succeeded,
        },
    }
}

fn validate_corpus(corpus: &Corpus) {
    let mut workspace_ids = BTreeSet::new();
    let mut query_ids = BTreeSet::new();
    for workspace in &corpus.workspaces {
        assert!(workspace_ids.insert(workspace.id.as_str()));
        assert!(!workspace.fixture.is_empty());
        assert!(
            workspace
                .queries
                .iter()
                .any(|query| query.kind == QueryKind::Exact)
        );
        assert!(
            workspace
                .queries
                .iter()
                .any(|query| query.kind == QueryKind::Paraphrased)
        );
        for query in &workspace.queries {
            assert!(query_ids.insert(query.id.as_str()));
            assert!(!query.query.trim().is_empty());
        }
    }
}

fn index_workspace(fixture: &WorkspaceFixture) {
    let budget = budget(256, 1);
    coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
        .expect("start retrieval evaluation index");
    finish_scan(fixture, budget, 256);

    for _ in 0..256 {
        let Some(source) = pending_source_paths(&fixture.index(), Path::new(""), 1)
            .expect("read pending evaluation source")
            .into_iter()
            .next()
        else {
            break;
        };
        coordinate_source_query(
            &fixture.root,
            &fixture.index(),
            Path::new(""),
            &[source],
            budget,
        )
        .expect("extract discovered retrieval source");
    }
    let coverage = manifest_coverage(&fixture.index()).expect("read evaluation index coverage");
    assert!(!coverage.scan_active);
    assert_eq!(coverage.pending, 0);
    assert_eq!(coverage.stale, 0);
}

fn finish_scan(fixture: &WorkspaceFixture, budget: CoordinatorBudget, maximum_calls: usize) {
    for _ in 0..maximum_calls {
        if !manifest_coverage(&fixture.index())
            .expect("read active evaluation scan")
            .scan_active
        {
            return;
        }
        coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
            .expect("continue retrieval evaluation scan");
    }
    panic!("retrieval evaluation scan exceeded its call bound");
}

fn retrieve(
    fixture: &WorkspaceFixture,
    scope: &Path,
    query: &str,
    budget: CoordinatorBudget,
) -> SourceSearchReport {
    retrieve_workspace_sources(
        &fixture.root,
        &fixture.index(),
        scope,
        &RetrievalRequest {
            active_workspace_id: fixture.id.clone(),
            lexical: SourceSearchRequest {
                query: query.to_owned(),
                path_prefix: None,
                limit: 10,
            },
        },
        budget,
    )
    .expect("run retrieval evaluation query")
}

fn score_query(
    quality: &mut QualityAccumulator,
    judgment: &QueryJudgment,
    hits: &[SourceSearchHit],
) {
    assert!(!judgment.id.is_empty());
    assert!(!judgment.relevant_paths.is_empty());
    quality.queries += 1;
    match judgment.kind {
        QueryKind::Exact => quality.exact_queries += 1,
        QueryKind::Paraphrased => quality.paraphrased_queries += 1,
    }
    quality.relevant += judgment.relevant_paths.len();
    let relevant = judgment
        .relevant_paths
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let recalled_at_5 = recalled_paths(hits, &relevant, 5);
    let recalled_at_10 = recalled_paths(hits, &relevant, 10);
    quality.recalled_at_5 += recalled_at_5;
    quality.recalled_at_10 += recalled_at_10;
    let reciprocal_rank = hits
        .iter()
        .position(|hit| relevant.contains(hit.candidate.path.as_str()))
        .map_or(0.0, |rank| 1.0 / (rank + 1) as f64);
    quality.reciprocal_rank += reciprocal_rank;
}

impl QualityAccumulator {
    fn finish(self) -> QualityMetrics {
        QualityMetrics {
            queries: self.queries,
            exact_queries: self.exact_queries,
            paraphrased_queries: self.paraphrased_queries,
            relevance_judgments: self.relevant,
            recall_at_5: ratio(self.recalled_at_5, self.relevant),
            recall_at_10: ratio(self.recalled_at_10, self.relevant),
            mrr: self.reciprocal_rank / self.queries as f64,
            wrong_workspace_rate: ratio(
                self.wrong_workspace_violations,
                self.workspace_scope_checks,
            ),
        }
    }
}

fn measure_recovery(fixture: &WorkspaceFixture, judgments: &WorkspaceJudgments) -> bool {
    let query = &judgments.queries[0].query;
    let before = hit_identity(&retrieve(fixture, Path::new(""), query, budget(256, 16)).hits);
    let canonical = fixture
        .root
        .join(crate::workspace::CONTROL_DIR)
        .join("state.sqlite");
    Connection::open(&canonical)
        .expect("open recovery sentinel store")
        .execute_batch(
            "CREATE TABLE recovery_sentinel(value TEXT NOT NULL) STRICT;
             INSERT INTO recovery_sentinel(value) VALUES ('retained');",
        )
        .expect("seed recovery sentinel");
    remove_index_files(&fixture.index());
    index_workspace(fixture);
    let after = hit_identity(&retrieve(fixture, Path::new(""), query, budget(256, 16)).hits);
    let retained: String = Connection::open(canonical)
        .expect("reopen recovery sentinel store")
        .query_row("SELECT value FROM recovery_sentinel", [], |row| row.get(0))
        .expect("read recovery sentinel");
    assert_eq!(after, before);
    retained == "retained"
}

fn measure_external_detection(fixture: &WorkspaceFixture) -> usize {
    fixture.write(
        "external/late.md",
        b"externalcheckpointtoken was added after the completed index scan\n",
    );
    for attempt in 1..=3 {
        let report = retrieve(
            fixture,
            Path::new("external"),
            "externalcheckpointtoken",
            budget(256, 16),
        );
        if report
            .hits
            .iter()
            .any(|hit| hit.candidate.path == "external/late.md")
        {
            return attempt;
        }
    }
    0
}

fn wrong_workspace_control_violated() -> bool {
    let first = WorkspaceFixture::empty("workspace-control-first");
    first.write("first.md", b"workspaceboundarycontrol first\n");
    first.initialize();
    index_workspace(&first);
    let second = WorkspaceFixture::empty("workspace-control-second");
    second.write("second.md", b"workspaceboundarycontrol second\n");
    second.initialize();
    index_workspace(&second);

    let result = retrieve_workspace_sources(
        &first.root,
        &second.index(),
        Path::new(""),
        &RetrievalRequest {
            active_workspace_id: first.id.clone(),
            lexical: SourceSearchRequest {
                query: "workspaceboundarycontrol".to_owned(),
                path_prefix: None,
                limit: 10,
            },
        },
        budget(16, 1),
    );
    match result {
        Ok(_) => true,
        Err(error) => {
            assert!(error.to_string().contains("not the active workspace index"));
            false
        }
    }
}

struct LargeRepositoryMetrics {
    max_metadata_per_call: usize,
    max_content_reads_per_call: usize,
    partial_retrieval_succeeded: bool,
}

fn measure_large_repository(file_count: usize) -> LargeRepositoryMetrics {
    assert!(file_count > 0);
    let fixture = WorkspaceFixture::empty("large-repository-workspace");
    for index in 0..file_count {
        fixture.write(
            &format!("large/source-{index:04}.md"),
            format!("bounded repository fixture {index}\n").as_bytes(),
        );
    }
    fixture.initialize();
    let budget = budget(16, 1);
    let first =
        coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
            .expect("start large-repository scan");
    assert_eq!(first.coverage, IndexCoverage::Partial);
    let selected = format!("large/source-{:04}.md", file_count - 1);
    let partial = retrieve(&fixture, Path::new(""), &selected, budget);
    let partial_retrieval_succeeded = partial
        .hits
        .iter()
        .any(|hit| hit.candidate.path == selected);

    let mut max_metadata = first.metrics.metadata_entries_reconciled;
    let mut max_content = first.metrics.content_reads;
    for _ in 0..file_count.saturating_add(32) {
        if !manifest_coverage(&fixture.index())
            .expect("read large-repository scan state")
            .scan_active
        {
            break;
        }
        let report =
            coordinate_source_query(&fixture.root, &fixture.index(), Path::new(""), &[], budget)
                .expect("continue large-repository scan");
        max_metadata = max_metadata.max(report.metrics.metadata_entries_reconciled);
        max_content = max_content.max(report.metrics.content_reads);
    }
    assert!(
        !manifest_coverage(&fixture.index())
            .expect("read completed large-repository scan")
            .scan_active,
        "large-repository scan must complete within its fixture bound"
    );
    LargeRepositoryMetrics {
        max_metadata_per_call: max_metadata,
        max_content_reads_per_call: max_content,
        partial_retrieval_succeeded,
    }
}

fn budget(max_metadata_entries: usize, max_on_demand: usize) -> CoordinatorBudget {
    CoordinatorBudget {
        max_metadata_entries,
        max_duration: Duration::from_secs(2),
        max_on_demand,
    }
}

fn recalled_paths(hits: &[SourceSearchHit], relevant: &BTreeSet<&str>, limit: usize) -> usize {
    hits.iter()
        .take(limit)
        .filter_map(|hit| {
            relevant
                .contains(hit.candidate.path.as_str())
                .then_some(&hit.candidate.path)
        })
        .collect::<BTreeSet<_>>()
        .len()
}

fn hit_identity(hits: &[SourceSearchHit]) -> Vec<(String, u32, u32, String)> {
    hits.iter()
        .map(|hit| {
            (
                hit.candidate.path.clone(),
                hit.candidate.anchor_start,
                hit.candidate.anchor_end,
                hit.candidate.content_sha256.clone(),
            )
        })
        .collect()
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn remove_index_files(index: &Path) {
    for path in [
        index.to_path_buf(),
        PathBuf::from(format!("{}-shm", index.display())),
        PathBuf::from(format!("{}-wal", index.display())),
    ] {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("remove recovery index {}: {error}", path.display()),
        }
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).expect("read checked-in retrieval fixture") {
        let entry = entry.expect("read checked-in retrieval fixture entry");
        let target = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("read checked-in retrieval fixture type")
            .is_dir()
        {
            fs::create_dir_all(&target).expect("create copied retrieval fixture directory");
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy checked-in retrieval fixture file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_fts5_regression_meets_checked_in_quality_and_operational_bounds() {
        let report = run_benchmark(2, 64);
        let policy: BaselinePolicy =
            serde_json::from_str(POLICY_JSON).expect("parse retrieval baseline policy");
        assert!(report.quality.recall_at_5 >= policy.regression.minimum_recall_at_5);
        assert!(report.quality.recall_at_10 >= policy.regression.minimum_recall_at_10);
        assert!(report.quality.mrr >= policy.regression.minimum_mrr);
        assert!(
            report.quality.wrong_workspace_rate <= policy.regression.maximum_wrong_workspace_rate
        );
        assert_eq!(report.operations.external_detection_queries, 1);
        assert!(report.operations.recovery_preserved_canonical_state);
        assert!(report.operations.partial_index_retrieval_succeeded);
        assert!(report.operations.large_repository_max_metadata_per_call <= 16);
        assert!(
            report
                .operations
                .large_repository_max_content_reads_per_call
                <= 16
        );
    }
}
