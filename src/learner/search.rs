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
    let mut ts =
        conn.prepare("SELECT topic FROM memory_topics WHERE memory_id=?1 ORDER BY topic")?;
    for row in values {
        let (no, id, title, scope, summary, state, note, cues) = row?;
        let cues: Vec<String> = serde_json::from_str(&cues)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let cues = indexed_labels(cues);
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
mod tests;
