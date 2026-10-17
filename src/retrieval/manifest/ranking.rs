//! Derived, rebuildable ranking-only view over the lexical source index.
//!
//! Repeated running headers and footers ("navigation lines") inflate lexical
//! scores for every page of a document even though they carry no topic-specific
//! evidence. The stored candidate bodies, anchors, and hashes stay untouched;
//! only `source_search_ranked` omits lines that repeat on most indexed chunks
//! of one document. Matching still happens against the original `source_search`
//! rows, so navigation-only hits remain findable but rank and qualify honestly.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::{Transaction, params};

use super::{ManifestError, sqlite};

/// A line must appear on at least this many indexed chunks of one document.
const MIN_REPEATED_CHUNKS: usize = 4;
/// ... and on at least this percentage of the document's indexed chunks.
const REPEATED_CHUNK_PERCENT: usize = 60;

/// Normalize one body line for cross-chunk identity comparison. Lines are
/// compared after whitespace collapsing and case folding; punctuation is kept
/// so a heading marker or separator does not merge distinct lines.
fn normalized_line(line: &str) -> Option<String> {
    let normalized = line
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

/// Lines that repeat on enough indexed chunks of one document to be treated as
/// possible navigation. Each chunk counts a normalized line at most once, so a
/// line repeated inside a single page never qualifies on its own.
pub(crate) fn navigation_lines(bodies: &[&str]) -> HashSet<String> {
    if bodies.len() < MIN_REPEATED_CHUNKS {
        return HashSet::new();
    }
    let mut counts: HashMap<String, usize> = HashMap::new();
    for body in bodies {
        let mut seen = HashSet::new();
        for line in body.lines() {
            if let Some(normalized) = normalized_line(line)
                && seen.insert(normalized.clone())
            {
                *counts.entry(normalized).or_insert(0) += 1;
            }
        }
    }
    counts
        .into_iter()
        .filter(|(_, count)| {
            *count >= MIN_REPEATED_CHUNKS && *count * 100 >= bodies.len() * REPEATED_CHUNK_PERCENT
        })
        .map(|(line, _)| line)
        .collect()
}

/// Body text used only for ranking: flagged navigation lines are omitted.
/// Original stored bodies are never rewritten.
pub(crate) fn ranking_body(body: &str, navigation: &HashSet<String>) -> String {
    body.lines()
        .filter(|line| {
            normalized_line(line).is_none_or(|normalized| !navigation.contains(&normalized))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn insert_ranked_candidate(
    transaction: &Transaction<'_>,
    index_path: &Path,
    candidate_no: i64,
    path: &str,
    structural_path: Option<&str>,
    ranked_body: &str,
    symbols: &str,
) -> Result<(), ManifestError> {
    transaction
        .execute(
            "INSERT INTO source_search_ranked(
                 rowid, path, structural_path, body, symbols
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![candidate_no, path, structural_path, ranked_body, symbols],
        )
        .map_err(|error| sqlite(index_path, "index ranked source candidate", error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MIN_REPEATED_CHUNKS, navigation_lines, ranking_body};

    #[test]
    fn repeated_navigation_requires_both_the_chunk_floor_and_share() {
        let footer = "Chapter 7 map: CLT · normal approximation · t distribution";
        let pages = |count: usize| {
            (0..count)
                .map(|index| format!("Body topic {index}\n{footer}\n"))
                .collect::<Vec<_>>()
        };
        let bodies = pages(6);
        let bodies = bodies.iter().map(String::as_str).collect::<Vec<_>>();
        let navigation = navigation_lines(&bodies);
        assert!(navigation.contains(&footer.to_lowercase()));

        let bodies = pages(MIN_REPEATED_CHUNKS - 1);
        let bodies = bodies.iter().map(String::as_str).collect::<Vec<_>>();
        assert!(
            navigation_lines(&bodies).is_empty(),
            "three chunks cannot establish a repeated line"
        );

        let mut bodies = pages(10);
        for (index, body) in bodies.iter_mut().enumerate() {
            if index % 2 == 0 {
                *body = format!("Unique body {index}\n");
            }
        }
        let bodies = bodies.iter().map(String::as_str).collect::<Vec<_>>();
        assert!(
            navigation_lines(&bodies).is_empty(),
            "five of ten chunks is below the sixty-percent share"
        );

        let bodies = pages(10);
        let bodies = bodies.iter().map(String::as_str).collect::<Vec<_>>();
        assert!(
            navigation_lines(&bodies).contains(&footer.to_lowercase()),
            "all ten chunks exceed both bounds"
        );
    }

    #[test]
    fn ranking_body_drops_only_flagged_lines_and_preserves_others() {
        let footer = "chapter 7 map: clt · normal approximation";
        let navigation = std::collections::HashSet::from([footer.to_owned()]);
        let body = "The CLT statement.\n  Chapter 7 map:   CLT · Normal Approximation  \nDetails.";
        let ranked = ranking_body(body, &navigation);
        assert!(!ranked.to_lowercase().contains("normal approximation"));
        assert!(ranked.contains("The CLT statement."));
        assert!(ranked.contains("Details."));
    }
}
