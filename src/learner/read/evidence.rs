//! Explicit evidence, session and historical reads from the caller's snapshot.
use super::{pages::*, records::*, requests::*};
use crate::learner::{model::SchemaVersion, search};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const SESSION_CHOICES_SQL: &str = "WITH pending AS MATERIALIZED (
    SELECT session_id,count(*) AS event_count,max(captured_at_ms) AS last_captured_at_ms
    FROM learning_events WHERE processing='pending' AND session_id IS NOT NULL GROUP BY session_id
) SELECT s.session_id,s.goal,s.status,s.revision,coalesce(p.event_count,0),p.last_captured_at_ms
FROM learning_sessions s LEFT JOIN pending p ON p.session_id=s.session_id
WHERE s.session_id>?1 AND (s.status IN ('active','paused') OR p.session_id IS NOT NULL)
ORDER BY s.session_id LIMIT 4";
pub(super) fn session_choice_rows(
    conn: &Connection,
    last: &str,
) -> Result<Vec<SessionChoice>, String> {
    let mut st = conn.prepare(SESSION_CHOICES_SQL).map_err(db)?;
    st.query_map([last], |r| {
        let id: String = r.get(0)?;
        Ok(SessionChoice {
            session_id: id.clone(),
            goal: r.get(1)?,
            status: r.get(2)?,
            revision: r.get(3)?,
            pending_event_count: r.get(4)?,
            last_pending_captured_at_ms: r.get(5)?,
            pending_events_ref: NextCall::pending_events(&id),
        })
    })
    .map_err(db)?
    .collect::<rusqlite::Result<_>>()
    .map_err(db)
}
pub(super) fn pending_notice(
    conn: &Connection,
    builder: &mut Builder,
    host: (&str, &str),
) -> Result<(), String> {
    let (workspace,current,unbound):(i64,i64,i64)=conn.query_row("SELECT count(*),coalesce(sum(host_kind=?1 AND host_thread_id=?2),0),coalesce(sum(session_id IS NULL),0) FROM learning_events WHERE processing='pending'",params![host.0,host.1],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(db)?;
    builder.required(|p| {
        p.pending_evidence = Some(PendingNotice {
            workspace_pending_event_count: workspace,
            current_host_pending_event_count: current,
            unbound_pending_event_count: unbound,
            interpretation_status: "uninterpreted",
            session_choices_ref: NextCall::session_choices(None),
            pending_work_ref: NextCall::pending_work(unbound > 0),
        })
    })
}
pub(super) fn session_context(
    conn: &Connection,
    selected: Option<&str>,
    builder: &mut Builder,
    host: (&str, &str),
) -> Result<(), String> {
    let bound: Option<String> = conn
        .query_row(
            "SELECT session_id FROM host_bindings WHERE host_kind=?1 AND host_thread_id=?2",
            params![host.0, host.1],
            |r| r.get(0),
        )
        .optional()
        .map_err(db)?
        .flatten();
    if let Some(id) = selected.or(bound.as_deref()) {
        let (session_id, goal, status, checkpoint, revision) = conn.query_row(
            "SELECT session_id,goal,status,checkpoint_json,revision FROM learning_sessions WHERE session_id=?1",
            [id],
            |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?, row.get::<_,String>(2)?, row.get::<_,String>(3)?, row.get::<_,i64>(4)?)),
        ).optional().map_err(db)?.ok_or_else(||"unknown_reference: session_id".to_string())?;
        let checkpoint = crate::learner::records::StoredCheckpoint::decode(&checkpoint, id)
            .map_err(|error| error.to_string())?;
        let basis = checkpoint_basis(conn, &checkpoint)?;
        let session = Session {
            session_id,
            goal,
            status,
            checkpoint,
            revision,
        };
        // The saved diagnosis and its bounded current dependency status are one
        // required envelope, before pending previews or optional result bodies.
        builder.required(|p| {
            p.session = Some(session);
            p.checkpoint_basis = Some(basis);
        })?;
        let mut st=conn.prepare("SELECT event_id FROM learning_events WHERE processing='pending' AND session_id=?1 ORDER BY event_no LIMIT 33").map_err(db)?;
        let events = st
            .query_map([id], |r| r.get(0))
            .map_err(db)?
            .collect::<rusqlite::Result<Vec<String>>>()
            .map_err(db)?;
        builder.preview_pending(events, id)
    } else {
        let rows = session_choice_rows(conn, "")?;
        let request = Request::SessionChoices(SessionChoicesRequest {
            schema: SchemaVersion::V2,
            max_bytes: None,
            limit: None,
            cursor: None,
        });
        let scope = CursorScope::new(&request, &builder.packet.workspace_id, Dependencies::Live);
        builder.preview_choices(rows, &scope)
    }
}
pub(super) fn read(
    conn: &Connection,
    r: &ReadRequest,
    builder: &mut Builder,
) -> Result<(), String> {
    let mut records = vec![];
    let mut last_fitting = None;
    for (index, reference) in r.refs.iter().enumerate() {
        super::check_read_cancellation()?;
        records.push(match reference {
            ReadRef::CurrentMemory {
                id,
                expected_revision,
            } => Record::Memory(complete_memory(conn, id, *expected_revision)?),
            ReadRef::Observation { id } => Record::Observation(observation(conn, id)?),
            ReadRef::Event {
                id,
                start_byte,
                end_byte,
            } => Record::Event(event(conn, id, *start_byte, *end_byte)?),
        });
        let remaining = &r.refs[index + 1..];
        let next_call = (!remaining.is_empty()).then(|| NextCall::selected_read(r, remaining));
        let next = selected_packet(builder, &records, next_call);
        // Try the complete requested range first: consuming its last byte drops
        // the same-event continuation, so terminal and partial envelopes differ.
        if size(&next) <= builder.max {
            last_fitting = Some(next);
            continue;
        }
        if let Some(Record::Event(event)) = records.last()
            && let Some(page) =
                event_page_packet(builder, r, &records[..records.len() - 1], event, remaining)?
        {
            return builder.required(|packet| *packet = page);
        }
        let mut without_continuation = next;
        without_continuation.next_call = None;
        // The no-continuation packet grows monotonically. Once it fails, no
        // longer prefix can fit, even when its remaining refs get shorter.
        if size(&without_continuation) > builder.max {
            break;
        }
    }
    if let Some(packet) = last_fitting {
        return builder.required(|next| *next = packet);
    }
    if matches!(r.refs.first(), Some(ReadRef::Event { .. })) {
        return Err(format!(
            "budget_too_small: event provenance, metadata, support notices, remaining references and a nonempty UTF-8 range do not fit; {}; no event bytes were returned",
            if builder.max < crate::learner::limits::PACKET_BYTES {
                "increase max_bytes (up to 65536) or narrow the selected refs"
            } else {
                "narrow the selected refs; mandatory event metadata cannot be omitted and raising max_bytes cannot help"
            }
        ));
    }
    // A single-record read retains exactly the accepted write-time envelope.
    // A multi-record request may need narrowing if its remaining refs consume
    // the space beside that record. Never silently drop a mandatory reference.
    let mut narrower = r.clone();
    narrower.max_bytes = Some(crate::learner::limits::PACKET_BYTES);
    if r.refs.len() == 1 && builder.max == crate::learner::limits::PACKET_BYTES {
        return Err("budget_too_small: whole record and mandatory metadata exceed the maximum packet; narrowing or increasing max_bytes cannot make this whole record readable".into());
    }
    let next_call = NextCall::selected_read(&narrower, &r.refs[..1]);
    Err(format!(
        "budget_too_small: next whole record and remaining references do not fit; retry this first reference, then the remaining original refs; next_call={}",
        serde_json::to_string(&next_call).map_err(|e| e.to_string())?
    ))
}

fn selected_packet(builder: &Builder, records: &[Record], next_call: Option<NextCall>) -> Packet {
    let mut packet = builder.packet.clone();
    packet.records = Some(records.to_vec());
    packet.next_call = next_call;
    packet
}

fn event_page_packet(
    builder: &Builder,
    request: &ReadRequest,
    earlier: &[Record],
    event: &Event,
    remaining: &[ReadRef],
) -> Result<Option<Packet>, String> {
    if let Some(packet) = event_prefix_packet(builder, request, earlier, event, remaining)? {
        return Ok(Some(packet));
    }
    if event.current_memory_refs.is_empty() {
        return Ok(None);
    }
    // Relationship previews can be deferred, but their existence and executable
    // support path remain explicit beside the original provenance and metadata.
    let mut deferred = event.clone();
    deferred.current_memory_refs.clear();
    deferred.current_memory_refs_more = true;
    deferred.current_memory_refs_continuation = Some(NextCall::support(
        ObservationKind::EventMemoryRefs,
        None,
        Some(event.event_id.clone()),
    ));
    let mut records = earlier.to_vec();
    records.push(Record::Event(deferred.clone()));
    let packet = selected_packet(
        builder,
        &records,
        (!remaining.is_empty()).then(|| NextCall::selected_read(request, remaining)),
    );
    if size(&packet) <= builder.max {
        return Ok(Some(packet));
    }
    event_prefix_packet(builder, request, earlier, &deferred, remaining)
}

/// Raw events alone have byte-range semantics. Keep every provenance/support
/// field and size the real packet plus its executable continuation at each probe.
fn event_prefix_packet(
    builder: &Builder,
    request: &ReadRequest,
    earlier: &[Record],
    event: &Event,
    remaining: &[ReadRef],
) -> Result<Option<Packet>, String> {
    let ends = event
        .text
        .char_indices()
        .map(|(offset, character)| offset + character.len_utf8())
        .take_while(|end| *end < event.text.len())
        .collect::<Vec<_>>();
    let mut low = 0;
    let mut high = ends.len();
    let mut fitting = None;
    // Among nonterminal prefixes, encoded text and endpoint widths only grow.
    // The complete/terminal candidate was checked separately by read().
    while low < high {
        super::check_read_cancellation()?;
        #[cfg(test)]
        crate::request::test_checkpoint("learner_event_page_probe");
        let middle = low + (high - low) / 2;
        let end = ends[middle];
        let mut prefix = event.clone();
        prefix.text = event.text[..end].into();
        prefix.end_byte = event.start_byte + end;
        prefix.partial = true;
        let mut refs = vec![ReadRef::Event {
            id: event.event_id.clone(),
            start_byte: Some(prefix.end_byte),
            end_byte: Some(event.end_byte),
        }];
        refs.extend_from_slice(remaining);
        let mut records = earlier.to_vec();
        records.push(Record::Event(prefix));
        let packet = selected_packet(
            builder,
            &records,
            Some(NextCall::selected_read(request, &refs)),
        );
        if size(&packet) <= builder.max {
            fitting = Some(packet);
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    super::check_read_cancellation()?;
    Ok(fitting)
}
fn numeric_after(scope: &CursorScope, request: &Request, default: i64) -> Result<i64, String> {
    match scope.after(request)? {
        None => Ok(default),
        Some(PageKey::Number(n)) if n >= 0 => Ok(n),
        _ => Err("invalid_cursor: numeric page key required".into()),
    }
}
pub(super) fn topics(
    conn: &Connection,
    request: &Request,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    let last = match scope.after(request)? {
        None => String::new(),
        Some(PageKey::Text(s)) => s,
        _ => return Err("invalid_cursor: topic key must be string".into()),
    };
    let limit = request.limit();
    let mut st=conn.prepare("SELECT t.topic,count(*) FROM memory_topics t JOIN learner_memories m ON m.memory_id=t.memory_id WHERE m.lifecycle='active' AND t.topic>?1 GROUP BY t.topic ORDER BY t.topic LIMIT ?2").map_err(db)?;
    let rows = st
        .query_map(params![last, (limit + 1) as i64], |r| {
            Ok(Topic {
                topic: r.get(0)?,
                count: r.get(1)?,
            })
        })
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    builder.required(|p| {
        p.topics = Some(vec![]);
        p.page_count = Some(0);
    })?;
    builder.whole_page(
        rows.into_iter()
            .map(|row| PageItem {
                after: PageKey::Text(row.topic.clone()),
                value: Item::Topic(row),
            })
            .collect(),
        limit,
        scope,
        || PageCoverage::None,
    )
}
pub(super) fn history(
    conn: &Connection,
    request: &Request,
    r: &HistoryRequest,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    let current = card(conn, &r.memory_id)?;
    let last = numeric_after(scope, request, i64::MAX)?;
    let limit = request.limit();
    let mut st=conn.prepare("SELECT revision,snapshot_json,change_kind,reason,change_basis_json,created_at_ms FROM memory_changes WHERE memory_id=?1 AND revision<?2 ORDER BY revision DESC LIMIT ?3").map_err(db)?;
    let rows = st
        .query_map(
            params![r.memory_id, last, (limit + 1) as i64],
            HistoricalChange::decode,
        )
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    builder.required(|p| {
        p.current_ref = Some(current_ref(&current));
        p.history = Some(vec![]);
        p.page_count = Some(0);
    })?;
    builder.whole_page(
        rows.into_iter()
            .map(|row| PageItem {
                after: PageKey::Number(row.revision),
                value: Item::History(row),
            })
            .collect(),
        limit,
        scope,
        || PageCoverage::None,
    )
}
pub(super) fn reviews(
    conn: &Connection,
    request: &Request,
    r: &ReviewsRequest,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    let last = match scope.after(request)? {
        None => String::new(),
        Some(PageKey::Text(s)) if !s.is_empty() && s.len() <= 256 => s,
        _ => return Err("invalid_cursor: review key required".into()),
    };
    let total:i64=conn.query_row("SELECT count(*) FROM review_items WHERE status='suggested' AND (?1 IS NULL OR memory_id=?1)",[&r.memory_id],|r|r.get(0)).map_err(db)?;
    let limit = request.limit();
    let mut st=conn.prepare("SELECT review_id,memory_id,memory_revision,purpose,reason,due_at_ms,status,revision,updated_at_ms FROM review_items WHERE status='suggested' AND review_id>?1 AND (?2 IS NULL OR memory_id=?2) ORDER BY review_id LIMIT ?3").map_err(db)?;
    let rows = st
        .query_map(params![last, r.memory_id, (limit + 1) as i64], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<i64>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
            ))
        })
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    let ids = rows.iter().map(|row| row.1.clone()).collect::<Vec<_>>();
    let items = rows
        .into_iter()
        .zip(cards(conn, &ids)?)
        .map(
            |(
                (
                    review_id,
                    memory_id,
                    memory_revision,
                    purpose,
                    reason,
                    due_at_ms,
                    stored_status,
                    revision,
                    updated_at_ms,
                ),
                current,
            )| {
                let status = if current.revision != memory_revision || !current.lifecycle.active() {
                    "needs_reconsideration"
                } else {
                    "suggested"
                };
                PageItem {
                    after: PageKey::Text(review_id.clone()),
                    value: Item::Review(Review {
                        review_id,
                        memory_id,
                        memory_revision,
                        purpose,
                        reason,
                        due_at_ms,
                        stored_status,
                        revision,
                        updated_at_ms,
                        status,
                        current_memory: current,
                    }),
                }
            },
        )
        .collect::<Vec<_>>();
    builder.required(|p| {
        p.reviews = Some(vec![]);
        p.page_count = Some(0);
        p.coverage.review_queue_count = Some(total);
        p.coverage.review_queue_complete = Some(last.is_empty() && items.is_empty());
    })?;
    builder.whole_page(items, limit, scope, || {
        PageCoverage::Review(last.is_empty())
    })?;
    let selected = builder
        .packet
        .reviews
        .as_ref()
        .unwrap()
        .iter()
        .map(|review| review.current_memory.clone())
        .collect::<Vec<_>>();
    let bodies = bodies(conn, &selected)?;
    for (index, card) in selected.iter().enumerate() {
        if crate::request::OperationCancellation::none().is_cancelled() {
            return Err("request_cancelled: learner read cancelled".into());
        }
        if let Some(body) = bodies.get(&card.memory_id) {
            builder.optional(|p| {
                let card = &mut p.reviews.as_mut().unwrap()[index].current_memory;
                card.state_text = Some(body.clone());
                card.detail_status = "complete";
            });
        }
    }
    Ok(())
}
pub(super) fn observations(
    conn: &Connection,
    request: &Request,
    r: &ObservationsRequest,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    if matches!(
        r.kind,
        ObservationKind::MemoryRefs | ObservationKind::EventRefs | ObservationKind::EventMemoryRefs
    ) {
        return support(conn, request, r, builder, scope);
    }
    let last = numeric_after(scope, request, 0)?;
    let limit = request.limit();
    let event_mode = r.kind == ObservationKind::Event;
    let ids: Vec<(i64, String)> = if event_mode {
        let mut st=conn.prepare("SELECT e.event_no,e.event_id FROM learning_events e WHERE e.event_no>?1 AND (?2 IS NULL OR e.session_id=?2) AND (?3 IS NULL OR EXISTS(SELECT 1 FROM observation_events oe JOIN memory_observations mo ON mo.observation_id=oe.observation_id WHERE oe.event_id=e.event_id AND mo.memory_id=?3)) AND EXISTS(SELECT 1 FROM json_each(?4) j WHERE length(j.value)>0 AND instr(e.text,j.value)>0) ORDER BY e.event_no LIMIT ?5").map_err(db)?;
        st.query_map(
            params![
                last,
                r.session_id,
                r.memory_id,
                serde_json::to_string(&r.literal_terms).unwrap(),
                (limit + 1) as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db)?
    } else {
        let compiled = r.query.as_ref().map(search::compile).transpose()?;
        if compiled.is_some() && !search::index_current(conn, true).map_err(db)? {
            builder.required(|p|{p.observations=Some(vec![]);p.page_count=Some(0);p.coverage.query_execution_complete=Some(false);p.warnings.push("index_unavailable: observation query not executed; retry without query to browse".into());})?;
            return Ok(());
        }
        let sql = if compiled.is_some() {
            "SELECT o.observation_no,o.observation_id FROM observations o JOIN observation_fts f ON f.rowid=o.observation_no WHERE o.observation_no>?1 AND (?2 IS NULL OR o.session_id=?2) AND (?3 IS NULL OR EXISTS(SELECT 1 FROM memory_observations mo WHERE mo.observation_id=o.observation_id AND mo.memory_id=?3)) AND observation_fts MATCH ?4 ORDER BY o.observation_no LIMIT ?5"
        } else {
            "SELECT o.observation_no,o.observation_id FROM observations o WHERE o.observation_no>?1 AND (?2 IS NULL OR o.session_id=?2) AND (?3 IS NULL OR EXISTS(SELECT 1 FROM memory_observations mo WHERE mo.observation_id=o.observation_id AND mo.memory_id=?3)) AND ?4 IS NULL ORDER BY o.observation_no LIMIT ?5"
        };
        let mut st = conn.prepare(sql).map_err(db)?;
        st.query_map(
            params![
                last,
                r.session_id,
                r.memory_id,
                compiled,
                (limit + 1) as i64
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(db)?
        .collect::<rusqlite::Result<_>>()
        .map_err(db)?
    };
    let items = ids
        .into_iter()
        .map(|(no, id)| {
            let value = if event_mode {
                let mut e = event(conn, &id, Some(0), Some(0))?;
                e.read_ref = Some(ReadRef::Event {
                    id,
                    start_byte: None,
                    end_byte: None,
                });
                Record::Event(e)
            } else {
                Record::Observation(observation(conn, &id)?)
            };
            Ok(PageItem {
                value: Item::Observation(value),
                after: PageKey::Number(no),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    builder.required(|p| {
        p.observations = Some(vec![]);
        p.page_count = Some(0);
    })?;
    builder.whole_page(items, limit, scope, || PageCoverage::None)
}
fn support(
    conn: &Connection,
    request: &Request,
    r: &ObservationsRequest,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    let event_memories = r.kind == ObservationKind::EventMemoryRefs;
    let memories = r.kind == ObservationKind::MemoryRefs || event_memories;
    let id = if event_memories {
        r.event_id.as_deref()
    } else {
        r.observation_id.as_deref()
    }
    .ok_or("invalid_variant: support reference required")?;
    let last = numeric_after(scope, request, 0)?;
    let limit = request.limit();
    let sql = if event_memories {
        "SELECT DISTINCT m.memory_no,m.memory_id FROM learner_memories m JOIN memory_observations mo ON mo.memory_id=m.memory_id JOIN observation_events oe ON oe.observation_id=mo.observation_id WHERE oe.event_id=?1 AND m.memory_no>?2 ORDER BY m.memory_no LIMIT ?3"
    } else if memories {
        "SELECT m.memory_no,m.memory_id FROM learner_memories m JOIN memory_observations mo ON mo.memory_id=m.memory_id WHERE mo.observation_id=?1 AND m.memory_no>?2 ORDER BY m.memory_no LIMIT ?3"
    } else {
        "SELECT e.event_no,e.event_id FROM learning_events e WHERE e.event_no>?2 AND EXISTS(SELECT 1 FROM observation_events oe WHERE oe.observation_id=?1 AND oe.event_id=e.event_id) ORDER BY e.event_no LIMIT ?3"
    };
    let mut st = conn.prepare(sql).map_err(db)?;
    let rows = st
        .query_map(params![id, last, (limit + 1) as i64], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    let items=rows.into_iter().map(|(no,event_id)| {let record=if memories{Record::Memory(card(conn,&event_id)?)}else {let mut e=event(conn,&event_id,Some(0),Some(0))?;e.read_ref=Some(ReadRef::Event {id:event_id.clone(),start_byte:None,end_byte:None});let mut roles=conn.prepare("SELECT role,excerpts_json FROM observation_events WHERE observation_id=?1 AND event_id=?2 ORDER BY role").map_err(db)?;e.observation_roles=Some(roles.query_map(params![r.observation_id,event_id],|r|Ok(ObservationRole {role:r.get(0)?,excerpts:stored(r.get(1)?)?})).map_err(db)?.collect::<rusqlite::Result<Vec<_>>>().map_err(db)?);Record::Event(e)};Ok(PageItem {value:Item::Support(record),after:PageKey::Number(no)})}).collect::<Result<Vec<_>,String>>()?;
    builder.required(|p| {
        p.support = Some(vec![]);
        p.page_count = Some(0);
    })?;
    builder.whole_page(items, limit, scope, || PageCoverage::None)
}
pub(super) fn session_choices(
    conn: &Connection,
    request: &Request,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    let last = match scope.after(request)? {
        None => String::new(),
        Some(PageKey::Text(s)) => s,
        _ => return Err("invalid_cursor: session key required".into()),
    };
    let rows = session_choice_rows(conn, &last)?;
    builder.required(|p| {
        p.session_choices.clear();
        p.page_count = Some(0);
    })?;
    builder.whole_page(
        rows.into_iter()
            .map(|row| PageItem {
                after: PageKey::Text(row.session_id.clone()),
                value: Item::SessionChoice(row),
            })
            .collect(),
        request.limit().min(3),
        scope,
        || PageCoverage::None,
    )
}
pub(super) fn pending_events(
    conn: &Connection,
    request: &Request,
    r: &PendingEventsRequest,
    builder: &mut Builder,
    scope: &CursorScope,
    host: (&str, &str),
) -> Result<(), String> {
    let last = match scope.after(request)? {
        None => 0,
        Some(PageKey::Pending(key))
            if key.host_kind == host.0 && key.host_thread_id == host.1 && key.event_no >= 0 =>
        {
            key.event_no
        }
        _ => return Err("invalid_cursor: native host/pending event key mismatch".into()),
    };
    let limit = request.limit();
    let mut st=conn.prepare("SELECT event_no,event_id FROM learning_events WHERE event_no>?1 AND processing='pending' AND ((?2 IS NOT NULL AND session_id=?2) OR (?2 IS NULL AND host_kind=?3 AND host_thread_id=?4)) ORDER BY event_no LIMIT ?5").map_err(db)?;
    let rows = st
        .query_map(
            params![last, r.session_id, host.0, host.1, (limit + 1) as i64],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    builder.required(|p| {
        p.pending_event_refs.clear();
        p.pending_events_more = Some(!rows.is_empty());
        p.page_count = Some(0);
    })?;
    builder.whole_page(
        rows.into_iter()
            .map(|(no, id)| PageItem {
                after: PageKey::Pending(PendingKey {
                    event_no: no,
                    host_kind: host.0.into(),
                    host_thread_id: host.1.into(),
                }),
                value: Item::Pending(PendingRef::Read(ReadRef::Event {
                    id,
                    start_byte: None,
                    end_byte: None,
                })),
            })
            .collect(),
        limit,
        scope,
        || PageCoverage::Pending,
    )
}

pub(super) fn pending_work(
    conn: &Connection,
    request: &Request,
    r: &PendingWorkRequest,
    builder: &mut Builder,
    scope: &CursorScope,
) -> Result<(), String> {
    let last = numeric_after(scope, request, 0)?;
    let limit = request.limit();
    let mut st=conn.prepare("SELECT event_no,event_id,session_id,host_kind,host_thread_id,origin,captured_at_ms FROM learning_events WHERE event_no>?1 AND processing='pending' AND (?2 IS NULL OR host_kind=?2) AND (?3 IS NULL OR host_thread_id=?3) AND (?4 IS NULL OR session_id=?4) AND (?5=0 OR session_id IS NULL) ORDER BY event_no LIMIT ?6").map_err(db)?;
    let rows = st
        .query_map(
            params![
                last,
                r.host_kind,
                r.host_thread_id,
                r.session_id,
                r.unbound_only,
                (limit + 1) as i64
            ],
            |row| {
                let id: String = row.get(1)?;
                Ok((
                    row.get::<_, i64>(0)?,
                    PendingWork {
                        event_id: id.clone(),
                        session_id: row.get(2)?,
                        host_kind: row.get(3)?,
                        host_thread_id: row.get(4)?,
                        origin: row.get(5)?,
                        captured_at_ms: row.get(6)?,
                        capture_time_meaning: "capture_time_not_occurrence_time",
                        read_ref: ReadRef::Event {
                            id,
                            start_byte: None,
                            end_byte: None,
                        },
                    },
                ))
            },
        )
        .map_err(db)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db)?;
    builder.required(|p| {
        p.pending_work = Some(vec![]);
        p.page_count = Some(0);
    })?;
    builder.whole_page(
        rows.into_iter()
            .map(|(no, row)| PageItem {
                value: Item::PendingWork(row),
                after: PageKey::Number(no),
            })
            .collect(),
        limit,
        scope,
        || PageCoverage::None,
    )
}
