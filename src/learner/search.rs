//! Deterministic lexical retrieval over canonical current descriptions.
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Query {
    #[serde(default)]
    /// Alternatives; every phrase is quoted literally and joined with OR.
    #[schemars(length(max = 12))]
    pub any_of: Vec<String>,
    #[serde(default)]
    /// Required literal phrases joined with AND; no raw FTS operators.
    #[schemars(length(max = 4))]
    pub all_of: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub id: String,
    pub purpose: String,
    #[serde(default)]
    #[schemars(length(max = 2))]
    pub queries: Vec<Query>,
}
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    pub id: String,
    pub score: f64,
    pub channels: BTreeSet<String>,
}
#[derive(Debug, Default)]
pub struct SearchResult {
    pub intents: Vec<(String, Vec<Candidate>)>,
    pub truncated: BTreeSet<String>,
    pub warnings: Vec<String>,
    pub original_routing: Option<String>,
}
pub fn normalize(value: &str) -> String {
    value
        .nfc()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
pub fn compile(query: &Query) -> Result<String, String> {
    if query.any_of.len() > 12 || query.all_of.len() > 4 {
        return Err("field_too_large: phrase array".into());
    }
    let clean = |phrases: &[String]| -> Result<Vec<String>, String> {
        phrases
            .iter()
            .map(|p| {
                if p.len() > 256 {
                    return Err("field_too_large: phrase".into());
                }
                let p = p.nfc().collect::<String>().trim().to_owned();
                Ok(p)
            })
            .collect()
    };
    let any = clean(&query.any_of)?;
    let all = clean(&query.all_of)?;
    if any
        .iter()
        .chain(&all)
        .all(|p| !p.chars().any(char::is_alphanumeric))
    {
        return Err("no_search_terms".into());
    }
    // A punctuation-only restriction cannot match unicode61: skip the formulation,
    // rather than silently broaden its meaning by removing that restriction.
    if all.iter().any(|p| !p.chars().any(char::is_alphanumeric)) {
        return Err("no_search_terms".into());
    }
    let mut parts = Vec::new();
    let any: Vec<_> = any
        .iter()
        .filter(|p| p.chars().any(char::is_alphanumeric))
        .map(|p| quote(p))
        .collect();
    if !any.is_empty() {
        parts.push(format!("({})", any.join(" OR ")));
    }
    parts.extend(all.iter().map(|p| quote(p)));
    Ok(parts.join(" AND "))
}
pub fn index_current(conn: &Connection, observation: bool) -> rusqlite::Result<bool> {
    let (table, generation, indexed) = if observation {
        (
            "observation_fts",
            "observation_generation",
            "observation_fts_generation",
        )
    } else {
        ("memory_fts", "memory_generation", "memory_fts_generation")
    };
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1 AND type='table')",
        [table],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(false);
    }
    conn.query_row(
        &format!("SELECT {generation}={indexed} FROM workspace_meta WHERE singleton=1"),
        [],
        |r| r.get(0),
    )
}
const ACTIVE_MEMORY_PROJECTION: &str = "SELECT memory_no,memory_id,title,scope,summary,state_text,review_note,retrieval_cues_json FROM learner_memories WHERE lifecycle='active'";

fn indexed_labels(labels: impl IntoIterator<Item = String>) -> String {
    labels
        .into_iter()
        .map(|label| normalize(&label))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Both selected-ID maintenance and rebuilding insert this same active-only
/// projection, including label normalization, deduplication, order, and NULLs.
fn insert_active_memories(conn: &Connection, memory_id: Option<&str>) -> rusqlite::Result<()> {
    let query = format!(
        "{ACTIVE_MEMORY_PROJECTION}{} ORDER BY memory_no",
        if memory_id.is_some() {
            " AND memory_id=?1"
        } else {
            ""
        }
    );
    let mut rows = conn.prepare(&query)?;
    let values = rows.query_map(rusqlite::params_from_iter(memory_id), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    for row in values {
        let (no, id, title, scope, summary, state, note, cues) = row?;
        let cues: Vec<String> = serde_json::from_str(&cues)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let cues = indexed_labels(cues);
        let mut ts =
            conn.prepare("SELECT topic FROM memory_topics WHERE memory_id=?1 ORDER BY topic")?;
        let topics = indexed_labels(
            ts.query_map([&id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?,
        );
        conn.execute("INSERT INTO memory_fts(rowid,memory_id,title,scope,topics,summary,state_text,review_note,retrieval_cues) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![no,id,title,scope,topics,summary,state,note,cues])?;
    }
    Ok(())
}

/// Called within the writer's existing index-maintenance savepoint. A retired
/// memory has its old document removed and no replacement inserted. Missing
/// canonical IDs fail maintenance so the existing dirty/rebuild path repairs
/// corruption instead of scanning every valid update by the unindexed text ID.
pub(crate) fn sync_memories(
    conn: &Connection,
    memory_ids: &BTreeSet<String>,
) -> rusqlite::Result<()> {
    for id in memory_ids {
        let rowid: i64 = conn.query_row(
            "SELECT memory_no FROM learner_memories WHERE memory_id=?1",
            [id],
            |row| row.get(0),
        )?;
        conn.execute("DELETE FROM memory_fts WHERE rowid=?1", [rowid])?;
        insert_active_memories(conn, Some(id))?;
    }
    Ok(())
}

/// Called inside a writer savepoint or maintenance transaction. Never changes canonical state.
pub fn rebuild_indexes(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(memory_id UNINDEXED,title,scope,topics,summary,state_text,review_note,retrieval_cues,tokenize='unicode61'); CREATE VIRTUAL TABLE IF NOT EXISTS observation_fts USING fts5(observation_id UNINDEXED,description,conditions_text,assistance_text,tokenize='unicode61'); DELETE FROM memory_fts; DELETE FROM observation_fts;")?;
    insert_active_memories(conn, None)?;
    conn.execute_batch("INSERT INTO observation_fts(rowid,observation_id,description,conditions_text,assistance_text) SELECT observation_no,observation_id,description,conditions_text,assistance_text FROM observations; UPDATE workspace_meta SET memory_fts_generation=memory_generation,observation_fts_generation=observation_generation WHERE singleton=1;")
}
pub fn reindex(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    match rebuild_indexes(conn) {
        Ok(()) => {
            if let Err(e) = conn.execute_batch("COMMIT") {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(e);
            }
            Ok(())
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// One handler covers every statement in the read snapshot, including initial
/// reference/count queries. Its lifetime ends before transaction cleanup so a
/// cancelled scan cannot leave a progress callback on a reused connection.
pub(crate) struct ReadProgress<'a> {
    conn: &'a Connection,
    exhausted: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
impl<'a> ReadProgress<'a> {
    pub(crate) fn install(conn: &'a Connection) -> Result<Self, String> {
        let exhausted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let limit = exhausted.clone();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let mut ticks = 0_usize;
        conn.progress_handler(
            1000,
            Some(move || {
                #[cfg(test)]
                crate::request::test_checkpoint("learner_read_progress");
                ticks += 1;
                let over_budget = ticks >= 10000 || std::time::Instant::now() >= deadline;
                limit.store(over_budget, std::sync::atomic::Ordering::Relaxed);
                over_budget || crate::request::OperationCancellation::none().is_cancelled()
            }),
        )
        .map_err(|e| e.to_string())?;
        Ok(Self { conn, exhausted })
    }
    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted.load(std::sync::atomic::Ordering::Relaxed)
    }
}
impl Drop for ReadProgress<'_> {
    fn drop(&mut self) {
        let _ = self.conn.progress_handler(0, None::<fn() -> bool>);
    }
}
#[cfg(test)]
pub(crate) fn search(
    conn: &Connection,
    supplied: &[Intent],
    original: &str,
    literals: &[String],
    literal_intent: Option<&str>,
) -> Result<SearchResult, String> {
    let _progress = ReadProgress::install(conn)?;
    search_in_snapshot(conn, supplied, original, literals, literal_intent)
}

/// Called by the read snapshot owner with its progress guard already installed.
/// Do not replace or clear that handler: it also bounds later projections.
pub(crate) fn search_in_snapshot(
    conn: &Connection,
    supplied: &[Intent],
    original: &str,
    literals: &[String],
    literal_intent: Option<&str>,
) -> Result<SearchResult, String> {
    if supplied.len() > 4
        || literals.len() > 4
        || literals.iter().any(|s| s.len() > 256)
        || original.len() > 2048
    {
        return Err("field_too_large: search".into());
    }
    let mut ids = BTreeSet::new();
    for i in supplied {
        if i.id.is_empty()
            || i.id.len() > 128
            || i.purpose.len() > 2048
            || i.queries.len() > 2
            || !ids.insert(&i.id)
        {
            return Err("invalid_variant: intent".into());
        }
    }
    let mut intents = supplied.to_vec();
    let mut result = SearchResult::default();
    let words: Vec<String> = original
        .nfc()
        .collect::<String>()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.chars().count() >= 2)
        .map(str::to_owned)
        .fold(Vec::new(), |mut a, s| {
            if a.len() < 12 && !a.contains(&s) {
                a.push(s)
            }
            a
        });
    if !words.is_empty() {
        if intents.is_empty() {
            intents.push(Intent {
                id: "original-wording".into(),
                purpose: "Original learner wording".into(),
                queries: vec![],
            });
        }
        result.original_routing = Some(intents[0].id.clone());
        intents[0].queries.push(Query {
            any_of: words,
            all_of: vec![],
        });
    }
    if !literals.is_empty() && intents.is_empty() {
        intents.push(Intent {
            id: "literal".into(),
            purpose: "Literal notation".into(),
            queries: vec![],
        });
    }
    let target = literal_intent
        .map(str::to_owned)
        .or_else(|| intents.first().map(|i| i.id.clone()));
    if literal_intent.is_some() && !intents.iter().any(|i| Some(&i.id) == target.as_ref()) {
        return Err("invalid_variant: literal_intent".into());
    }
    let current = index_current(conn, false).map_err(|e| e.to_string())?;
    if !current {
        result
            .warnings
            .push("index_unavailable: use literal matching or browse".into());
    }
    #[cfg(test)]
    crate::request::test_checkpoint("learner_ranking");
    let run = (|| {
        let mut cache: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for intent in intents {
            let mut fused: BTreeMap<String, Candidate> = BTreeMap::new();
            let mut seen = BTreeSet::new();
            for query in &intent.queries {
                let compiled = match compile(query) {
                    Ok(q) => q,
                    Err(e) if e == "no_search_terms" => {
                        result
                            .warnings
                            .push(format!("no_search_terms: {}", intent.id));
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                if !current {
                    continue;
                }
                if !seen.insert(compiled.clone()) {
                    continue;
                }
                // Each formulation matches one complete current record. Required
                // terms may live in different fields, while FTS phrases remain
                // within a field. A formulation supplies one rank-fusion vote,
                // even when both descriptive text and retrieval cues match it.
                let rows = if let Some(rows) = cache.get(&compiled) {
                    rows.clone()
                } else {
                    let mut st=conn.prepare("SELECT m.memory_id FROM memory_fts JOIN learner_memories m ON m.memory_no=memory_fts.rowid WHERE memory_fts MATCH ?1 AND m.lifecycle='active' ORDER BY bm25(memory_fts,0,5,2,2,4,1,1,1),m.memory_id LIMIT 25").map_err(|e|e.to_string())?;
                    let rows = st
                        .query_map([&compiled], |r| r.get::<_, String>(0))
                        .map_err(|e| e.to_string())?
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map_err(|e| e.to_string())?;
                    cache.insert(compiled, rows.clone());
                    rows
                };
                if rows.len() > 24 {
                    result.truncated.insert(intent.id.clone());
                }
                for (rank, id) in rows.into_iter().take(24).enumerate() {
                    let c = fused.entry(id.clone()).or_insert_with(|| Candidate {
                        id,
                        ..Default::default()
                    });
                    c.score += 1.0 / (61 + rank) as f64;
                    // This is record-level lexical eligibility, not evidence
                    // that the vocabulary appeared in an actual learner answer.
                    c.channels.insert("lexical".into());
                }
            }
            if Some(&intent.id) == target.as_ref() && !literals.is_empty() {
                let encoded = serde_json::to_string(literals).map_err(|e| e.to_string())?;
                let mut st=conn.prepare("WITH hits AS (SELECT m.memory_id,m.memory_no,0 AS field,instr(m.title,j.value) AS pos FROM learner_memories m,json_each(?1) j WHERE m.lifecycle='active' AND length(j.value)>0 UNION ALL SELECT m.memory_id,m.memory_no,1,instr(m.scope,j.value) FROM learner_memories m,json_each(?1) j WHERE m.lifecycle='active' AND length(j.value)>0 UNION ALL SELECT m.memory_id,m.memory_no,2,instr(m.summary,j.value) FROM learner_memories m,json_each(?1) j WHERE m.lifecycle='active' AND length(j.value)>0 UNION ALL SELECT m.memory_id,m.memory_no,3,instr(m.state_text,j.value) FROM learner_memories m,json_each(?1) j WHERE m.lifecycle='active' AND length(j.value)>0), ranked AS (SELECT *,row_number() OVER(PARTITION BY memory_id ORDER BY field,pos) AS n FROM hits WHERE pos>0) SELECT memory_id FROM ranked WHERE n=1 ORDER BY field,pos,memory_no LIMIT 25").map_err(|e|e.to_string())?;
                let rows = st
                    .query_map([encoded], |r| r.get::<_, String>(0))
                    .map_err(|e| e.to_string())?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|e| e.to_string())?;
                if rows.len() > 24 {
                    result.truncated.insert(intent.id.clone());
                }
                for (rank, id) in rows.into_iter().take(24).enumerate() {
                    let c = fused.entry(id.clone()).or_insert_with(|| Candidate {
                        id,
                        ..Default::default()
                    });
                    c.score += 1.0 / (61 + rank) as f64;
                    c.channels.insert("literal".into());
                }
            }
            let mut candidates: Vec<_> = fused.into_values().collect();
            candidates.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
            result.intents.push((intent.id, candidates));
        }
        Ok(())
    })();
    if crate::request::OperationCancellation::none().is_cancelled() {
        return Err("request_cancelled: learner search cancelled".into());
    }
    match run {
        Ok(()) => Ok(result),
        Err(e) if e.contains("interrupted") => {
            result
                .warnings
                .push("query_cancelled: incomplete coverage; browse or narrow query".into());
            Ok(result)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(crate::learner::store::SCHEMA_SQL).unwrap();
        c.execute("INSERT INTO workspace_meta(singleton,workspace_id,storage_family,schema_version,created_at_ms) VALUES(1,'w-test','learner-model/v2',?1,0)",[super::super::store::SCHEMA_VERSION]).unwrap();
        c
    }
    pub fn memory(c: &Connection, id: &str, title: &str, state: &str, cues: &str) {
        c.execute("INSERT INTO learner_memories(memory_id,title,scope,summary,state_text,retrieval_cues_json,current_basis_json,lifecycle,revision,created_at_ms,updated_at_ms) VALUES(?1,?2,'algebra',?3,?3,?4,'[]','active',1,0,0)",params![id,title,state,cues]).unwrap();
    }
    #[test]
    fn whole_record_formulation_matches_alias_with_required_content() {
        let c = fixture();
        memory(
            &c,
            "quadratic",
            "Quadratic factorization",
            "Can expand; reversing expansion needs support.",
            r#"["undo FOIL"]"#,
        );
        rebuild_indexes(&c).unwrap();
        let intent = Intent {
            id: "reverse-expansion".into(),
            purpose: "Find the relevant factorization description".into(),
            queries: vec![Query {
                any_of: vec!["undo FOIL".into()],
                all_of: vec!["quadratic".into()],
            }],
        };
        let result = search(&c, &[intent], "", &[], None).unwrap();
        assert_eq!(
            result.intents[0]
                .1
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["quadratic"],
            "the cue phrase and required title term belong to one current record"
        );
        assert_eq!(
            result.intents[0].1[0].channels,
            BTreeSet::from(["lexical".into()])
        );
    }
    #[test]
    fn whole_record_formulation_matches_description_with_required_cue() {
        let c = fixture();
        memory(
            &c,
            "quadratic",
            "Factorization",
            "Can expand; reversing expansion needs support.",
            r#"["quadratic"]"#,
        );
        rebuild_indexes(&c).unwrap();
        let mut intent = Intent {
            id: "reverse-expansion".into(),
            purpose: "Find the relevant factorization description".into(),
            queries: vec![Query {
                any_of: vec!["reversing expansion".into()],
                all_of: vec!["quadratic".into()],
            }],
        };
        let result = search(&c, std::slice::from_ref(&intent), "", &[], None).unwrap();
        assert_eq!(result.intents[0].1[0].id, "quadratic");
        intent.queries[0].all_of.push("unavailable".into());
        let result = search(&c, &[intent], "", &[], None).unwrap();
        assert!(
            result.intents[0].1.is_empty(),
            "every required phrase must match somewhere in the same record"
        );
    }
    #[test]
    fn whole_record_formulation_does_not_form_phrases_across_fields() {
        let c = fixture();
        memory(&c, "split", "undo", "FOIL", r#"["quadratic"]"#);
        memory(
            &c,
            "phrase",
            "Factorization",
            "reversing expansion",
            r#"["undo FOIL", "quadratic"]"#,
        );
        rebuild_indexes(&c).unwrap();
        let intent = Intent {
            id: "reverse-expansion".into(),
            purpose: "Keep literal phrase semantics".into(),
            queries: vec![Query {
                any_of: vec!["undo FOIL".into()],
                all_of: vec!["quadratic".into()],
            }],
        };
        let result = search(&c, &[intent], "", &[], None).unwrap();
        assert_eq!(result.intents[0].1.len(), 1);
        assert_eq!(result.intents[0].1[0].id, "phrase");
    }
    #[test]
    fn whole_record_formulation_preserves_unicode_and_quoted_operators() {
        let c = fixture();
        memory(
            &c,
            "unicode",
            "Coefficient notation",
            "Uses x² − b in a café example.",
            r#"["cafe\u0301"]"#,
        );
        memory(&c, "quoted", "a OR b", "coefficient", "[]");
        memory(&c, "operator-decoy", "b", "coefficient", "[]");
        rebuild_indexes(&c).unwrap();
        let intents = [
            Intent {
                id: "unicode".into(),
                purpose: "NFC normalization across fields".into(),
                queries: vec![Query {
                    any_of: vec!["cafe\u{301}".into()],
                    all_of: vec!["coefficient".into()],
                }],
            },
            Intent {
                id: "quoted".into(),
                purpose: "Quoted input cannot broaden a formulation".into(),
                queries: vec![Query {
                    any_of: vec!["a\" OR b".into()],
                    all_of: vec!["coefficient".into()],
                }],
            },
        ];
        let result = search(&c, &intents, "", &["x² − b".into()], Some("unicode")).unwrap();
        assert_eq!(result.intents[0].1.len(), 1);
        assert_eq!(result.intents[0].1[0].id, "unicode");
        assert_eq!(
            result.intents[0].1[0].channels,
            BTreeSet::from(["lexical".into(), "literal".into()])
        );
        assert_eq!(result.intents[1].1.len(), 1);
        assert_eq!(result.intents[1].1[0].id, "quoted");
    }
    #[test]
    fn whole_record_formulation_keeps_bounds_and_active_row_filter() {
        let c = fixture();
        for n in 0..26 {
            memory(&c, &format!("m-{n:02}"), "coefficient", "evidence", "[]");
        }
        memory(&c, "retired", "coefficient", "evidence", "[]");
        rebuild_indexes(&c).unwrap();
        // Even a stale document cannot bypass current canonical lifecycle.
        c.execute(
            "UPDATE learner_memories SET lifecycle='retired' WHERE memory_id='retired'",
            [],
        )
        .unwrap();
        let intent = Intent {
            id: "roles".into(),
            purpose: "Bounded active lexical results".into(),
            queries: vec![Query {
                any_of: vec!["coefficient".into()],
                all_of: vec![],
            }],
        };
        let result = search(&c, &[intent], "", &[], None).unwrap();
        assert_eq!(result.intents[0].1.len(), 24);
        assert_eq!(result.truncated, BTreeSet::from(["roles".into()]));
        assert!(result.intents[0].1.iter().all(|c| c.id != "retired"));
    }
    #[test]
    fn literal_fts_operators_are_quoted_and_or_is_explicit() {
        let c = fixture();
        memory(&c, "a", "factoring", "does not confuse coefficients", "[]");
        memory(&c, "b", "factorization", "coefficient labels unclear", "[]");
        memory(&c, "c", "undo FOIL", "multiply binomials", "[]");
        rebuild_indexes(&c).unwrap();
        let i = Intent {
            id: "test".into(),
            purpose: "alternatives".into(),
            queries: vec![Query {
                any_of: vec![
                    "factoring".into(),
                    "factorization".into(),
                    "undo FOIL".into(),
                ],
                all_of: vec![],
            }],
        };
        let s = search(&c, std::slice::from_ref(&i), "", &[], None).unwrap();
        assert_eq!(s.intents[0].1.len(), 3);
        let mut restricted = i;
        restricted.queries[0].all_of.push("coefficient".into());
        assert_eq!(
            search(&c, &[restricted], "", &[], None).unwrap().intents[0]
                .1
                .len(),
            1
        );
        assert_eq!(
            compile(&Query {
                any_of: vec!["a\" OR b".into()],
                all_of: vec![]
            })
            .unwrap(),
            "(\"a\"\" OR b\")"
        );
        assert_eq!(
            compile(&Query {
                any_of: vec!["!!!".into()],
                all_of: vec![]
            })
            .unwrap_err(),
            "no_search_terms"
        );
    }
    #[test]
    fn cues_current_rows_literal_and_dirty_fallback() {
        let c = fixture();
        memory(
            &c,
            "a",
            "symbol roles",
            "x² − b is not a negative sign error",
            "[\"coefficient\"]",
        );
        memory(&c, "b", "coefficient", "old error", "[]");
        c.execute(
            "UPDATE learner_memories SET lifecycle='retired' WHERE memory_id='b'",
            [],
        )
        .unwrap();
        rebuild_indexes(&c).unwrap();
        let i = Intent {
            id: "roles".into(),
            purpose: "retrieval cue".into(),
            queries: vec![Query {
                any_of: vec!["coefficient".into()],
                all_of: vec![],
            }],
        };
        let s = search(&c, std::slice::from_ref(&i), "", &[], None).unwrap();
        assert_eq!(s.intents[0].1.len(), 1);
        assert_eq!(
            s.intents[0].1[0].channels,
            BTreeSet::from(["lexical".to_string()])
        );
        let s = search(&c, &[], "", &["x² − b".into()], None).unwrap();
        assert_eq!(s.intents[0].1[0].id, "a");
        c.execute("UPDATE workspace_meta SET memory_generation=1", [])
            .unwrap();
        assert!(!index_current(&c, false).unwrap());
        let s = search(&c, &[i], "", &[], None).unwrap();
        assert!(s.intents[0].1.is_empty());
        assert!(!s.warnings.is_empty());
        reindex(&c).unwrap();
        assert!(index_current(&c, false).unwrap());
    }
    #[test]
    fn duplicate_queries_do_not_inflate_ranks() {
        let c = fixture();
        memory(&c, "a", "coefficient", "fixed labels", r#"["coefficient"]"#);
        rebuild_indexes(&c).unwrap();
        let q = Query {
            any_of: vec!["coefficient".into()],
            all_of: vec![],
        };
        let mut i = Intent {
            id: "roles".into(),
            purpose: "roles".into(),
            queries: vec![q.clone()],
        };
        let one = search(&c, std::slice::from_ref(&i), "", &[], None)
            .unwrap()
            .intents[0]
            .1[0]
            .score;
        assert_eq!(one, 1.0 / 61.0, "one formulation supplies one vote");
        i.queries.push(q);
        assert_eq!(
            search(&c, &[i], "", &[], None).unwrap().intents[0].1[0].score,
            one
        );
    }
    #[test]
    fn distinct_formulations_fuse_within_each_intent() {
        let c = fixture();
        memory(&c, "a", "coefficient", "fixed labels", r#"["coefficient"]"#);
        rebuild_indexes(&c).unwrap();
        let query = Query {
            any_of: vec!["coefficient".into()],
            all_of: vec![],
        };
        let result = search(
            &c,
            &[
                Intent {
                    id: "two-formulations".into(),
                    purpose: "Two distinct matching opportunities".into(),
                    queries: vec![
                        query.clone(),
                        Query {
                            any_of: vec!["fixed labels".into()],
                            all_of: vec![],
                        },
                    ],
                },
                Intent {
                    id: "one-formulation".into(),
                    purpose: "The shared cache does not combine intents".into(),
                    queries: vec![query],
                },
            ],
            "",
            &[],
            None,
        )
        .unwrap();
        assert_eq!(result.intents[0].1[0].score, 2.0 / 61.0);
        assert_eq!(result.intents[1].1[0].score, 1.0 / 61.0);
    }

    #[test]
    fn selected_sync_work_does_not_scale_with_fts_corpus_size() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let count_update = |size: usize| {
            let c = fixture();
            for index in 0..size {
                memory(
                    &c,
                    &format!("m-{index:05}"),
                    "coefficient",
                    "evidence",
                    "[]",
                );
            }
            rebuild_indexes(&c).unwrap();
            c.execute(
                "UPDATE learner_memories SET title='revised coefficient' WHERE memory_id='m-00050'",
                [],
            )
            .unwrap();
            let steps = Arc::new(AtomicUsize::new(0));
            let counter = steps.clone();
            c.progress_handler(
                1,
                Some(move || {
                    counter.fetch_add(1, Ordering::Relaxed);
                    false
                }),
            )
            .unwrap();
            sync_memories(&c, &BTreeSet::from(["m-00050".into()])).unwrap();
            c.progress_handler(0, None::<fn() -> bool>).unwrap();
            assert_eq!(
                c.query_row("SELECT count(*) FROM memory_fts", [], |row| row
                    .get::<_, i64>(0))
                    .unwrap(),
                size as i64
            );
            assert_eq!(c.query_row("SELECT title FROM memory_fts WHERE rowid=(SELECT memory_no FROM learner_memories WHERE memory_id='m-00050')", [], |row| row.get::<_, String>(0)).unwrap(), "revised coefficient");
            steps.load(Ordering::Relaxed)
        };
        let small = count_update(100);
        let large = count_update(10_000);
        println!("selected sync VM steps: 100 memories={small}, 10,000 memories={large}");
        assert!(
            large <= small * 3,
            "one selected update scanned the growing FTS corpus: 100 rows={small} VM steps, 10,000 rows={large} VM steps"
        );
    }

    #[test]
    fn missing_canonical_memory_requires_rebuild_instead_of_unindexed_cleanup() {
        let c = fixture();
        memory(&c, "a", "coefficient", "current evidence", "[]");
        rebuild_indexes(&c).unwrap();
        c.execute("INSERT INTO memory_fts(rowid,memory_id,title) VALUES(99,'missing','stale coefficient')", []).unwrap();
        assert!(matches!(
            sync_memories(&c, &BTreeSet::from(["missing".into()])),
            Err(rusqlite::Error::QueryReturnedNoRows)
        ));
        rebuild_indexes(&c).unwrap();
        assert_eq!(
            c.query_row(
                "SELECT group_concat(memory_id) FROM memory_fts",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "a"
        );
    }

    #[test]
    fn selected_sync_uses_rebuild_normalization_and_removes_retired_documents() {
        let c = fixture();
        let raw_cues = r#"["  ZOË\n","zoë","cafe\u0301","Café"]"#;
        memory(&c, "a", "Original CASE", "coefficient evidence", raw_cues);
        memory(&c, "b", "coefficient", "old evidence", "[]");
        for topic in ["  BETA ", "beta", "Cafe\u{301}", "CAFÉ"] {
            c.execute("INSERT INTO memory_topics VALUES('a',?1)", [topic])
                .unwrap();
        }
        rebuild_indexes(&c).unwrap();
        c.execute(
            "UPDATE learner_memories SET lifecycle='retired' WHERE memory_id='b'",
            [],
        )
        .unwrap();
        sync_memories(&c, &BTreeSet::from(["a".into(), "b".into()])).unwrap();
        let projection = || {
            c.query_row("SELECT json_group_array(json_array(rowid,memory_id,title,scope,topics,summary,state_text,review_note,retrieval_cues)) FROM (SELECT rowid,* FROM memory_fts ORDER BY rowid)", [], |row| row.get::<_, String>(0)).unwrap()
        };
        let before = projection();
        assert_eq!(
            c.query_row(
                "SELECT title,topics,retrieval_cues,review_note FROM memory_fts",
                [],
                |row| Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?
                ))
            )
            .unwrap(),
            (
                "Original CASE".into(),
                "beta\ncafé".into(),
                "café\nzoë".into(),
                None
            )
        );
        rebuild_indexes(&c).unwrap();
        assert_eq!(before, projection());
        assert_eq!(
            c.query_row("SELECT count(*) FROM memory_fts", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            c.query_row(
                "SELECT retrieval_cues_json FROM learner_memories WHERE memory_id='a'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            raw_cues
        );
    }
}
