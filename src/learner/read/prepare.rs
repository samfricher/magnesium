//! Select complete, fully annotated cards before packet admission.
use super::{evidence, pages::*, records::*, requests::*};
use crate::learner::search;
use rusqlite::Connection;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
thread_local! {
    pub(super) static RANKED_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn directory(
    conn: &Connection,
    request: &Request,
    topic: Option<&str>,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    let last = match scope.after(request)? {
        None => 0,
        Some(PageKey::Number(n)) if n >= 0 => n,
        _ => return Err("invalid_cursor: numeric directory key required".into()),
    };
    let topic = topic.map(search::normalize);
    let limit = request.limit();
    let rows = page_ids(conn, topic.as_deref(), last, limit + 1)?;
    builder.required(|p| {
        p.selection_mode = "browse".into();
        p.page_count = Some(0);
        p.coverage.catalog_complete = last == 0 && topic.is_none() && rows.is_empty();
    })?;
    let ids = rows.iter().map(|(_, id)| id.clone()).collect::<Vec<_>>();
    let items = rows
        .into_iter()
        .zip(cards(conn, &ids)?)
        .map(|((no, _), card)| PageItem {
            value: Item::Memory(card),
            after: PageKey::Number(no),
        })
        .collect();
    builder.whole_page(items, limit, scope, || {
        PageCoverage::Catalog(last == 0 && topic.is_none())
    })
}
fn browse_from_prepare(
    conn: &Connection,
    r: &PrepareRequest,
    builder: &mut Builder,
    source_scope: &CursorScope,
) -> Result<(), String> {
    // Fallback has a single public action/filter identity. The emitted call is
    // this real browse request, including its original budget and topic.
    let request = Request::Browse(BrowseRequest {
        schema: r.schema,
        max_bytes: r.max_bytes,
        limit: r.limit,
        cursor: r.cursor.clone(),
        topic: r.topic.clone(),
    });
    let scope = source_scope.retarget(&request);
    directory(conn, &request, r.topic.as_deref(), builder, &scope)
}
pub(super) fn enrich_bodies(conn: &Connection, builder: &mut Builder) -> Result<(), String> {
    let mut bodies = bodies(conn, &builder.packet.memories)?;
    for index in 0..builder.packet.memories.len() {
        check_cancelled()?;
        if let Some(body) = bodies.remove(&builder.packet.memories[index].memory_id) {
            builder.optional(|p| {
                p.memories[index].state_text = Some(body);
                p.memories[index].detail_status = "complete";
            });
        }
    }
    Ok(())
}
pub(super) fn prepare(
    conn: &Connection,
    request: &Request,
    r: &PrepareRequest,
    builder: &mut Builder,
    scope: &CursorScope,
    host: (&str, &str),
) -> Result<(), String> {
    let mut selected = BTreeSet::new();
    let priority_ids = r
        .memory_ids
        .iter()
        .filter(|id| selected.insert((*id).clone()))
        .cloned()
        .collect::<Vec<_>>();
    let mut ids = priority_ids.clone();
    ids.extend(
        r.known_revisions
            .iter()
            .map(|known| known.memory_id.clone()),
    );
    let mut loaded = cards(conn, &ids)?.into_iter();
    let mut priorities = loaded.by_ref().take(priority_ids.len()).collect::<Vec<_>>();
    let mut notices = vec![];
    for (known, card) in r.known_revisions.iter().zip(loaded) {
        let unchanged = card.revision == known.revision && card.lifecycle.active();
        notices.push(RevisionNotice {
            memory_id: known.memory_id.clone(),
            revision: card.revision,
            status: if unchanged {
                "unchanged"
            } else if card.lifecycle.active() {
                "changed"
            } else {
                "retired"
            },
            review_note: (!unchanged).then_some(card.review_note).flatten(),
            read_ref: (!unchanged).then_some(card.read_ref),
            replacement_ids: (!unchanged).then_some(card.replacement_ids),
        });
    }
    let mode = r.mode.unwrap_or(if matches!(request, Request::Search(_)) {
        Mode::Focused
    } else {
        Mode::Auto
    });
    if mode == Mode::Overview {
        builder.required(|p| {
            p.memories = priorities;
            p.revision_notices = notices;
        })?;
        evidence::session_context(conn, request.session_id(), builder, host)?;
        return browse_from_prepare(conn, r, builder, scope);
    }
    if mode == Mode::Auto {
        let mut directory = builder.clone();
        directory.required(|p| {
            p.memories = priorities.clone();
            p.revision_notices = notices.clone();
            p.selection_mode = "complete_directory".into();
            p.coverage.catalog_complete = true;
        })?;
        evidence::session_context(conn, request.session_id(), &mut directory, host)?;
        directory.max = directory
            .max
            .min(size(&directory.packet).saturating_add(12288));
        let mut last = 0;
        let mut complete = true;
        'outer: loop {
            let rows = page_ids(conn, None, last, 64)?;
            if rows.is_empty() {
                break;
            }
            let ids = rows.iter().map(|(_, id)| id.clone()).collect::<Vec<_>>();
            for ((no, id), card) in rows.into_iter().zip(cards(conn, &ids)?) {
                check_cancelled()?;
                last = no;
                if selected.contains(&id) {
                    continue;
                }
                if !directory.optional(|p| p.memories.push(card)) {
                    complete = false;
                    break 'outer;
                }
            }
        }
        if complete {
            directory.max = builder.max;
            *builder = directory;
            enrich_bodies(conn, builder)?;
            return Ok(());
        }
    }
    #[cfg(test)]
    RANKED_CALLS.with(|calls| calls.set(calls.get() + 1));
    let result = search::search_in_snapshot(
        conn,
        &r.intents,
        &r.original_query,
        &r.literal_terms,
        r.literal_intent.as_deref(),
    )?;
    let mut matches: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for (intent, list) in &result.intents {
        for candidate in list {
            let entry = matches.entry(candidate.id.clone()).or_default();
            entry.0.insert(intent.clone());
            entry.1.extend(candidate.channels.iter().cloned());
        }
    }
    let annotate = |card: &mut MemoryCard| {
        if let Some((intents, channels)) = matches.get(&card.memory_id) {
            card.matched_intents = intents.iter().cloned().collect();
            card.match_channels = channels.iter().cloned().collect();
        }
    };
    for card in &mut priorities {
        annotate(card);
    }
    let mut ranked_selected = selected.clone();
    let mut offsets = vec![0; result.intents.len()];
    let mut ordered = vec![];
    loop {
        let mut added = false;
        for (index, (_, list)) in result.intents.iter().enumerate() {
            while offsets[index] < list.len() && ranked_selected.contains(&list[offsets[index]].id)
            {
                offsets[index] += 1
            }
            if let Some(candidate) = list.get(offsets[index]) {
                ranked_selected.insert(candidate.id.clone());
                ordered.push(candidate.id.clone());
                offsets[index] += 1;
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    // Every fixed field and priority annotation is known before session
    // previews, optional ranked cards, or full bodies compete for space.
    let continuations = vec![NextCall::browse(r.topic.clone()), NextCall::topics()];
    let hints = result.truncated.iter().map(|id| format!(
        "Intent {id}: narrow or reformulate this intent, or browse; ranking is not exhaustive."
    )).collect();
    builder.required(|p| {
        p.memories = priorities;
        p.revision_notices = notices;
        p.selection_mode = "focused".into();
        p.coverage.ranked_retrieval_used = true;
        p.coverage.query_execution_complete = Some(
            !result
                .warnings
                .iter()
                .any(|w| w.starts_with("query_cancelled")),
        );
        p.coverage.candidate_lists_truncated = !result.truncated.is_empty();
        p.coverage.omitted_card_count = ordered.len();
        p.warnings = result.warnings.clone();
        p.original_wording_intent = result.original_routing.clone();
        p.continuations = continuations;
        p.hints = hints;
    })?;
    evidence::session_context(conn, request.session_id(), builder, host)?;
    let mut omitted = vec![];
    for mut card in cards(conn, &ordered)? {
        check_cancelled()?;
        annotate(&mut card);
        let read_ref = ReadRef::CurrentMemory {
            id: card.memory_id.clone(),
            expected_revision: Some(card.revision),
        };
        // These are optional ranked candidates, not directory keys. A rejected
        // candidate must not prevent another intent's later whole card fitting.
        if builder.packet.memories.len() >= 16
            || !builder.optional(|p| {
                p.memories.push(card);
                p.coverage.omitted_card_count -= 1;
            })
        {
            omitted.push(read_ref);
        }
    }
    // Selection gets first claim on space; references get a bounded preview
    // before optional bodies. The count includes every actual omission.
    for reference in omitted.into_iter().take(8) {
        if !builder.optional(|p| p.omitted_memory_refs.push(reference)) {
            break;
        }
    }
    if builder.packet.memories.is_empty() {
        if !ordered.is_empty() {
            return Err("budget_too_small: no ranked whole card fits; increase max_bytes or read it explicitly".into());
        }
        browse_from_prepare(conn, r, builder, scope)?;
    }
    enrich_bodies(conn, builder)
}

fn check_cancelled() -> Result<(), String> {
    if crate::request::OperationCancellation::none().is_cancelled() {
        Err("request_cancelled: learner read cancelled".into())
    } else {
        Ok(())
    }
}
