use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::{Value, json};

use super::request::{
    BlockWrite, CarryWrite, CourseMapRequest, ItemKind, ItemWrite, LinkWrite, ScopeState,
    ScopeWrite, TargetFacts, TraceWrite,
};
use super::store::{bump, fail};

pub(crate) const MAX_TITLE: usize = 200;
pub(crate) const MAX_NOTE: usize = 600;
const MAX_ITEMS_PER_WRITE: usize = 800;
const PAGE_CODE_EXTENSIONS: &[&str] = &["css", "scss", "less", "js", "mjs", "cjs", "map"];
const PAGE_CODE_FOLDERS: &[&str] = &["_assets", "site_libs", "libs", "assets", "static", "vendor"];

pub(crate) struct Source<'a> {
    pub(crate) root: &'a Path,
    pub(crate) index: &'a Path,
    pub(crate) now_ms: i64,
}

#[derive(Default)]
pub(crate) struct Report {
    pub(crate) files_added: Vec<Value>,
    pub(crate) items_added: Vec<Value>,
    pub(crate) items_changed: usize,
    pub(crate) items_removed: usize,
    pub(crate) links_added: usize,
    pub(crate) links_removed: usize,
    pub(crate) scope_rows: usize,
    pub(crate) blocks: Option<usize>,
    pub(crate) carried: Option<usize>,
    pub(crate) traces: usize,
    pub(crate) warnings: Vec<String>,
    pub(crate) targets: BTreeSet<String>,
}

impl Report {
    pub(crate) fn to_json(&self, revision: i64) -> Value {
        let mut value = json!({"revision": revision});
        if !self.files_added.is_empty() {
            value["files_added"] = json!(self.files_added);
        }
        if !self.items_added.is_empty() {
            value["items_added"] = json!(self.items_added);
        }
        for (key, count) in [
            ("items_changed", self.items_changed),
            ("items_removed", self.items_removed),
            ("links_added", self.links_added),
            ("links_removed", self.links_removed),
            ("scope_rows", self.scope_rows),
            ("traces", self.traces),
        ] {
            if count > 0 {
                value[key] = json!(count);
            }
        }
        if let Some(blocks) = self.blocks {
            value["blocks"] = json!(blocks);
        }
        if let Some(carried) = self.carried {
            value["carried"] = json!(carried);
        }
        if !self.warnings.is_empty() {
            value["warnings"] = json!(self.warnings);
        }
        value
    }
}

pub(crate) fn clean_text(text: &str, limit: usize) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(limit).collect()
}

pub(crate) fn valid_slug(slug: &str) -> Result<String, String> {
    let slug = slug.trim().to_lowercase();
    if slug.is_empty()
        || slug.len() > 40
        || !slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        || slug.starts_with(['-', '_', '.'])
    {
        return Err(format!(
            "target {slug:?} is not a short slug: use lowercase letters, digits and hyphens, such as test-2"
        ));
    }
    Ok(slug)
}

fn valid_code(code: &str) -> Option<String> {
    let code = code.trim().to_lowercase();
    (!code.is_empty()
        && code.len() <= 16
        && code
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !code.starts_with('-')
        && !matches!(code.as_str(), "found" | "outline" | "task" | "lesson"))
    .then_some(code)
}

fn valid_ref(text: &str) -> Result<String, String> {
    let cleaned = text.trim().to_lowercase().replace(' ', "");
    if cleaned.is_empty()
        || cleaned.len() > 24
        || !cleaned
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
        || cleaned.starts_with('.')
        || cleaned.ends_with('.')
    {
        return Err(format!(
            "ref {text:?} must be up to 24 letters, digits, dots or hyphens, such as 6.63 or q2"
        ));
    }
    Ok(cleaned)
}

pub(crate) fn code_from_path(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let words = stem
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>();
    let joined = words.join("-");
    let code = if joined.len() <= 16 || words.len() < 2 {
        joined.chars().take(16).collect::<String>()
    } else {
        let last = words[words.len() - 1];
        let first = words[0];
        let room = 16usize.saturating_sub(last.len() + 1).max(3);
        format!("{}-{last}", first.chars().take(room).collect::<String>())
            .chars()
            .take(16)
            .collect()
    };
    let code = code.trim_matches('-').to_owned();
    if code.is_empty() || valid_code(&code).is_none() {
        "file".to_owned()
    } else {
        code
    }
}

fn free_code(tx: &Transaction<'_>, wanted: &str) -> Result<String, String> {
    let taken = |code: &str| -> Result<bool, String> {
        tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE code = ?1)",
            [code],
            |r| r.get(0),
        )
        .map_err(fail)
    };
    if !taken(wanted)? {
        return Ok(wanted.to_owned());
    }
    for n in 2.. {
        let candidate = format!("{wanted}-{n}");
        if !taken(&candidate)? {
            return Ok(candidate);
        }
    }
    unreachable!()
}

pub(crate) fn file_code(tx: &Transaction<'_>, reference: &str) -> Result<Option<String>, String> {
    let reference = reference.trim().trim_start_matches("./");
    tx.query_row(
        "SELECT code FROM files WHERE code = ?1 OR path = ?2",
        params![reference.to_lowercase(), reference],
        |r| r.get(0),
    )
    .optional()
    .map_err(fail)
}

fn id_taken(tx: &Transaction<'_>, id: &str) -> Result<bool, String> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM items WHERE id = ?1) OR EXISTS(SELECT 1 FROM retired WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(fail)
}

fn item_exists(tx: &Transaction<'_>, id: &str) -> Result<bool, String> {
    tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM items WHERE id = ?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(fail)
}

fn need_item(tx: &Transaction<'_>, id: &str) -> Result<String, String> {
    let id = id.trim().to_lowercase();
    if item_exists(tx, &id)? {
        Ok(id)
    } else {
        Err(format!("no item {id:?} on the map"))
    }
}

fn page_code(path: &str) -> bool {
    let extension = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    PAGE_CODE_EXTENSIONS.contains(&extension.as_str())
        || path.split('/').rev().skip(1).any(|part| {
            PAGE_CODE_FOLDERS.contains(&part) || part.ends_with("_files") || part == ".magnesium"
        })
}

fn relative_path(path: &str) -> Option<std::path::PathBuf> {
    if path.is_empty() || path.chars().count() > 256 || path.chars().any(char::is_control) {
        return None;
    }
    let relative = std::path::PathBuf::from(path);
    (!relative.is_absolute()
        && relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))))
    .then_some(relative)
}

fn heading_parts(heading: &str) -> Vec<String> {
    heading
        .split(" > ")
        .map(|part| clean_text(part, MAX_TITLE))
        .filter(|part| !part.is_empty())
        .collect()
}

fn refresh_file(
    tx: &Transaction<'_>,
    source: &Source<'_>,
    code: &str,
    path: &str,
    report: &mut Report,
) -> Result<(), String> {
    let (readable, sha, items): (bool, Option<String>, i64) = tx
        .query_row(
            "SELECT readable = 1, content_sha256, (SELECT count(*) FROM items WHERE file = ?1)
             FROM files WHERE code = ?1",
            [code],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(fail)?;
    if (readable && sha.is_some()) || items > 0 || !source.index.is_file() {
        return Ok(());
    }
    let Some(relative) = relative_path(path) else {
        return Ok(());
    };
    let Some((row, sections)) =
        crate::retrieval::sections_at(source.index, &relative).map_err(fail)?
    else {
        return Ok(());
    };
    let now_readable =
        row.index_status == "indexed" && matches!(row.source_kind.as_str(), "text" | "pdf");
    if !now_readable {
        return Ok(());
    }
    let kind = if row.source_kind == "pdf" {
        "page"
    } else {
        "line"
    };
    tx.execute(
        "UPDATE files SET readable = 1, content_sha256 = ?2, anchor_kind = ?3 WHERE code = ?1",
        params![code, row.content_sha256, kind],
    )
    .map_err(fail)?;
    let count = insert_sections(
        tx,
        code,
        row.content_sha256.as_deref(),
        &sections.sections,
        source.now_ms,
    )?;
    report.files_added.push(json!({
        "path": path,
        "code": code,
        "sections": count,
        "note": "indexed since it was added: sections from the file's own headings",
    }));
    Ok(())
}

pub(crate) fn add_file(
    tx: &Transaction<'_>,
    source: &Source<'_>,
    path: &str,
    code: Option<&str>,
    report: &mut Report,
) -> Result<String, String> {
    let path = path.trim().trim_start_matches("./").to_owned();
    if let Some(existing) = file_code(tx, &path)? {
        refresh_file(tx, source, &existing, &path, report)?;
        return Ok(existing);
    }
    let relative = relative_path(&path)
        .ok_or_else(|| format!("{path:?} is not a path inside this study folder"))?;
    let absolute = source.root.join(&relative);
    crate::workspace::reject_symlink_components(&absolute, source.root).map_err(fail)?;
    let found = if source.index.is_file() {
        crate::retrieval::sections_at(source.index, &relative).map_err(fail)?
    } else {
        None
    };
    if found.is_none() && !absolute.is_file() {
        return Err(format!("no file {path:?} in this study folder"));
    }
    let wanted = match code {
        Some(code) => valid_code(code).ok_or_else(|| {
            format!("code {code:?} must be up to 16 lowercase letters, digits or hyphens")
        })?,
        None => code_from_path(&path),
    };
    let code = free_code(tx, &wanted)?;
    let (sha, kind, readable, sections) = match &found {
        Some((row, sections)) => (
            row.content_sha256.clone(),
            (row.source_kind == "pdf")
                .then_some("page")
                .or((row.source_kind == "text").then_some("line")),
            row.index_status == "indexed" && matches!(row.source_kind.as_str(), "text" | "pdf"),
            sections.sections.clone(),
        ),
        None => (None, None, false, Vec::new()),
    };
    tx.execute(
        "INSERT INTO files(code, path, content_sha256, anchor_kind, readable, added_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![code, path, sha, kind, readable as i64, source.now_ms],
    )
    .map_err(fail)?;
    let count = insert_sections(tx, &code, sha.as_deref(), &sections, source.now_ms)?;
    let note = if !readable {
        "the runtime cannot read this file as text: read it by eye and add its items"
    } else if sections.is_empty() {
        "the runtime found no section titles: read it by eye and add its items"
    } else {
        "sections from the file's own headings"
    };
    report.files_added.push(json!({
        "path": path,
        "code": code,
        "sections": count,
        "note": note,
    }));
    Ok(code)
}

pub(crate) fn insert_sections(
    tx: &Transaction<'_>,
    code: &str,
    sha: Option<&str>,
    sections: &[crate::retrieval::SectionFact],
    now_ms: i64,
) -> Result<usize, String> {
    let mut paths: Vec<Vec<String>> = sections.iter().map(|s| heading_parts(&s.heading)).collect();
    let shared_root = paths.len() > 1
        && paths.iter().all(|p| p.len() > 1)
        && paths.windows(2).all(|pair| pair[0][0] == pair[1][0]);
    if shared_root {
        for parts in &mut paths {
            parts.remove(0);
        }
    }
    let mut open: Vec<(String, String)> = Vec::new();
    let mut spans: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    let mut position = 0i64;
    let mut next = 0u32;
    let mut made = 0usize;
    for (section, parts) in sections.iter().zip(&paths) {
        let shared = open
            .iter()
            .zip(parts)
            .take_while(|((title, _), part)| title == *part)
            .count();
        open.truncate(shared);
        for part in &parts[shared..] {
            let id = loop {
                next += 1;
                let candidate = format!("{code}.{next}");
                if !id_taken(tx, &candidate)? {
                    break candidate;
                }
            };
            position += 1;
            let parent = open.last().map(|(_, id)| id.clone());
            tx.execute(
                "INSERT INTO items(id, file, parent, kind, origin, title, anchor_start, anchor_end, position, content_sha256, added_ms)
                 VALUES (?1, ?2, ?3, 'section', 'extracted', ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, code, parent, part, section.start, section.end, position, sha, now_ms],
            )
            .map_err(fail)?;
            made += 1;
            open.push((part.clone(), id));
        }
        for (_, id) in &open {
            let span = spans
                .entry(id.clone())
                .or_insert((section.start, section.end));
            span.0 = span.0.min(section.start);
            span.1 = span.1.max(section.end);
        }
    }
    for (id, (start, end)) in &spans {
        tx.execute(
            "UPDATE items SET anchor_start = ?2, anchor_end = ?3 WHERE id = ?1",
            params![id, start, end],
        )
        .map_err(fail)?;
    }
    Ok(made)
}

pub(crate) fn add_folder(
    tx: &Transaction<'_>,
    source: &Source<'_>,
    folder: &str,
    report: &mut Report,
) -> Result<Vec<String>, String> {
    let prefix = folder
        .trim()
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_owned()
        + "/";
    let files = if source.index.is_file() {
        crate::retrieval::indexed_files(source.index).map_err(fail)?
    } else {
        Vec::new()
    };
    let mut codes = Vec::new();
    for file in files.iter().filter(|f| f.path.starts_with(&prefix)) {
        if page_code(&file.path)
            || matches!(
                file.exclusion_reason.as_deref(),
                Some(
                    "sensitive"
                        | "sensitive_content"
                        | "sensitive_extracted"
                        | "magnesiumignore"
                        | "symlink"
                )
            )
        {
            continue;
        }
        codes.push(add_file(tx, source, &file.path, None, report)?);
    }
    if codes.is_empty() {
        report
            .warnings
            .push(format!("no files under {prefix} in the index yet"));
    }
    Ok(codes)
}

fn next_child(tx: &Transaction<'_>, base: &str) -> Result<String, String> {
    let prefix = format!("{base}.");
    let mut statement = tx
        .prepare("SELECT id FROM items WHERE id LIKE ?1 || '%' UNION SELECT id FROM retired WHERE id LIKE ?1 || '%'")
        .map_err(fail)?;
    let highest = statement
        .query_map([&prefix], |r| r.get::<_, String>(0))
        .map_err(fail)?
        .filter_map(Result::ok)
        .filter_map(|id| {
            id.strip_prefix(&prefix)
                .and_then(|rest| rest.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);
    let mut n = highest + 1;
    loop {
        let id = format!("{base}.{n}");
        if !id_taken(tx, &id)? {
            return Ok(id);
        }
        n += 1;
    }
}

fn next_position(
    tx: &Transaction<'_>,
    file: Option<&str>,
    parent: Option<&str>,
) -> Result<i64, String> {
    let after_parent: Option<i64> = match parent {
        Some(parent) => {
            let (own, last_child): (i64, Option<i64>) = tx
                .query_row(
                    "WITH RECURSIVE under(id) AS (SELECT id FROM items WHERE parent = ?1
                       UNION SELECT items.id FROM items JOIN under ON items.parent = under.id)
                     SELECT (SELECT position FROM items WHERE id = ?1),
                            (SELECT max(position) FROM items WHERE id IN (SELECT id FROM under))",
                    [parent],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(fail)?;
            Some(last_child.unwrap_or(own))
        }
        None => None,
    };
    match (after_parent, file) {
        (Some(after), Some(file)) => {
            tx.execute(
                "UPDATE items SET position = position + 1 WHERE file = ?1 AND position > ?2",
                params![file, after],
            )
            .map_err(fail)?;
            Ok(after + 1)
        }
        (Some(after), None) => {
            tx.execute(
                "UPDATE items SET position = position + 1 WHERE file IS NULL AND position > ?1",
                [after],
            )
            .map_err(fail)?;
            Ok(after + 1)
        }
        (None, Some(file)) => tx
            .query_row(
                "SELECT coalesce(max(position), 0) + 1 FROM items WHERE file = ?1",
                [file],
                |r| r.get(0),
            )
            .map_err(fail),
        (None, None) => tx
            .query_row(
                "SELECT coalesce(max(position), 0) + 1 FROM items WHERE file IS NULL",
                [],
                |r| r.get(0),
            )
            .map_err(fail),
    }
}

fn origin_for(item: &ItemWrite, kind: ItemKind, has_file: bool) -> Result<&'static str, String> {
    match item.origin.as_deref().map(str::trim) {
        Some("read") => Ok("read"),
        Some("outline") => Ok("outline"),
        Some("written") => Ok("written"),
        Some("lesson") => Ok("lesson"),
        Some("foundation") => Ok(if has_file { "read" } else { "lesson" }),
        Some(other) => Err(format!(
            "origin {other:?} is not one of read, outline, written, lesson"
        )),
        None if kind == ItemKind::Task => Ok("written"),
        None if has_file => Ok("read"),
        None if kind == ItemKind::Foundation => Ok("lesson"),
        None => Ok("outline"),
    }
}

fn add_item(
    tx: &Transaction<'_>,
    source: &Source<'_>,
    item: &ItemWrite,
    report: &mut Report,
) -> Result<String, String> {
    let kind = item
        .kind
        .ok_or("a new item needs kind: section, idea, exercise, question, example, reading, foundation or task")?;
    let title = item
        .title
        .as_deref()
        .map(|t| clean_text(t, MAX_TITLE))
        .filter(|t| !t.is_empty())
        .ok_or("a new item needs a title")?;
    let parent = item
        .parent
        .as_deref()
        .map(|p| need_item(tx, p))
        .transpose()?;
    let mut file = match item.file.as_deref() {
        Some(reference) => Some(match file_code(tx, reference)? {
            Some(code) => code,
            None => add_file(tx, source, reference, None, report)?,
        }),
        None => None,
    };
    if file.is_none()
        && let Some(parent) = &parent
    {
        file = tx
            .query_row("SELECT file FROM items WHERE id = ?1", [parent], |r| {
                r.get(0)
            })
            .map_err(fail)?;
    }
    let origin = origin_for(item, kind, file.is_some())?;
    let base = match (&parent, &file) {
        (Some(parent), _) => parent.clone(),
        (None, Some(code)) => code.clone(),
        (None, None) => match (kind, origin) {
            (ItemKind::Task, _) | (_, "written") => "task".to_owned(),
            (ItemKind::Foundation, _) => "found".to_owned(),
            (_, "lesson") => "lesson".to_owned(),
            _ => "outline".to_owned(),
        },
    };
    let reference = item.r#ref.as_deref().map(valid_ref).transpose()?;
    let id = match reference.as_deref() {
        Some(reference) => {
            let id = format!("{base}.{reference}");
            if item_exists(tx, &id)? {
                return Err(format!(
                    "item {id:?} is already on the map; give its id to change it"
                ));
            }
            if id_taken(tx, &id)? {
                return Err(format!(
                    "item {id:?} was on the map before and was removed; its answers stay with it, so give the new item another ref"
                ));
            }
            id
        }
        None => next_child(tx, &base)?,
    };
    let sha: Option<String> = match &file {
        Some(code) => tx
            .query_row(
                "SELECT content_sha256 FROM files WHERE code = ?1",
                [code],
                |r| r.get(0),
            )
            .map_err(fail)?,
        None => None,
    };
    let position = next_position(tx, file.as_deref(), parent.as_deref())?;
    let note = item
        .note
        .as_deref()
        .map(|n| clean_text(n, MAX_NOTE))
        .filter(|n| !n.is_empty());
    let (start, end) = anchors(item.anchor_start, item.anchor_end)?;
    tx.execute(
        "INSERT INTO items(id, file, parent, kind, origin, title, ref, note, anchor_start, anchor_end, position, content_sha256, added_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![id, file, parent, kind.as_str(), origin, title, reference, note, start, end, position, sha, source.now_ms],
    )
    .map_err(fail)?;
    report.items_added.push(json!({"id": id, "title": title}));
    Ok(id)
}

fn anchors(start: Option<u32>, end: Option<u32>) -> Result<(Option<u32>, Option<u32>), String> {
    match (start, end) {
        (Some(0), _) => Err("anchor_start counts from 1".into()),
        (Some(start), Some(end)) if end < start => Err("anchor_end is before anchor_start".into()),
        (None, Some(_)) => Err("anchor_end needs anchor_start".into()),
        (start, end) => Ok((start, end.or(start))),
    }
}

fn change_item(tx: &Transaction<'_>, id: &str, item: &ItemWrite) -> Result<(), String> {
    let id = need_item(tx, id)?;
    if let Some(kind) = item.kind {
        tx.execute(
            "UPDATE items SET kind = ?2 WHERE id = ?1",
            params![id, kind.as_str()],
        )
        .map_err(fail)?;
    }
    if let Some(title) = item
        .title
        .as_deref()
        .map(|t| clean_text(t, MAX_TITLE))
        .filter(|t| !t.is_empty())
    {
        tx.execute(
            "UPDATE items SET title = ?2 WHERE id = ?1",
            params![id, title],
        )
        .map_err(fail)?;
    }
    if let Some(note) = item.note.as_deref() {
        let note = clean_text(note, MAX_NOTE);
        tx.execute(
            "UPDATE items SET note = ?2 WHERE id = ?1",
            params![id, (!note.is_empty()).then_some(note)],
        )
        .map_err(fail)?;
    }
    if item.anchor_start.is_some() || item.anchor_end.is_some() {
        let (start, end) = anchors(item.anchor_start, item.anchor_end)?;
        tx.execute(
            "UPDATE items SET anchor_start = ?2, anchor_end = ?3 WHERE id = ?1",
            params![id, start, end],
        )
        .map_err(fail)?;
    }
    if let Some(parent) = item.parent.as_deref() {
        let parent = need_item(tx, parent)?;
        let cycle: bool = tx
            .query_row(
                "WITH RECURSIVE up(id) AS (SELECT ?2 UNION SELECT items.parent FROM items JOIN up ON items.id = up.id WHERE items.parent IS NOT NULL)
                 SELECT EXISTS(SELECT 1 FROM up WHERE id = ?1)",
                params![id, parent],
                |r| r.get(0),
            )
            .map_err(fail)?;
        if cycle {
            return Err(format!("{parent} is inside {id}, so it cannot hold it"));
        }
        tx.execute(
            "UPDATE items SET parent = ?2 WHERE id = ?1",
            params![id, parent],
        )
        .map_err(fail)?;
    }
    Ok(())
}

fn link(
    tx: &Transaction<'_>,
    write: &LinkWrite,
    add: bool,
    report: &mut Report,
) -> Result<(), String> {
    let item = need_item(tx, &write.item)?;
    for used in &write.uses {
        let used = need_item(tx, used)?;
        if used == item {
            continue;
        }
        let changed = if add {
            tx.execute(
                "INSERT OR IGNORE INTO uses(item, uses, origin) VALUES (?1, ?2, 'plan')",
                params![item, used],
            )
        } else {
            tx.execute(
                "DELETE FROM uses WHERE item = ?1 AND uses = ?2",
                params![item, used],
            )
        }
        .map_err(fail)?;
        if add {
            report.links_added += changed;
        } else {
            report.links_removed += changed;
        }
    }
    Ok(())
}

fn set_facts(
    tx: &Transaction<'_>,
    slug: &str,
    facts: &TargetFacts,
    now: i64,
) -> Result<(), String> {
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM targets WHERE slug = ?1)",
            [slug],
            |r| r.get(0),
        )
        .map_err(fail)?;
    if !exists {
        let label = facts
            .label
            .as_deref()
            .map(|l| clean_text(l, MAX_TITLE))
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| slug.to_owned());
        tx.execute(
            "INSERT INTO targets(slug, label, updated_ms) VALUES (?1, ?2, ?3)",
            params![slug, label, now],
        )
        .map_err(fail)?;
    }
    let text = |value: &Option<String>| value.as_deref().map(|v| clean_text(v, MAX_NOTE));
    if let Some(label) = text(&facts.label).filter(|l| !l.is_empty()) {
        tx.execute(
            "UPDATE targets SET label = ?2 WHERE slug = ?1",
            params![slug, label],
        )
        .map_err(fail)?;
    }
    if let Some(at) = text(&facts.at) {
        if !at.is_empty()
            && chrono::NaiveDate::parse_from_str(at.get(..10).unwrap_or(""), "%Y-%m-%d").is_err()
        {
            return Err(format!("at {at:?} must start with the date as YYYY-MM-DD"));
        }
        tx.execute(
            "UPDATE targets SET at = ?2 WHERE slug = ?1",
            params![slug, (!at.is_empty()).then_some(at)],
        )
        .map_err(fail)?;
    }
    if let Some(hours) = facts.hours {
        if !hours.is_finite() || hours < 0.0 {
            return Err("hours must be a positive number".into());
        }
        tx.execute(
            "UPDATE targets SET hours = ?2 WHERE slug = ?1",
            params![slug, hours],
        )
        .map_err(fail)?;
    }
    if let Some(minutes) = facts.length_minutes {
        tx.execute(
            "UPDATE targets SET length_minutes = ?2 WHERE slug = ?1",
            params![slug, minutes],
        )
        .map_err(fail)?;
    }
    if let Some(aids) = text(&facts.aids) {
        tx.execute(
            "UPDATE targets SET aids = ?2 WHERE slug = ?1",
            params![slug, aids],
        )
        .map_err(fail)?;
    }
    if let Some(goal) = text(&facts.goal) {
        tx.execute(
            "UPDATE targets SET goal = ?2 WHERE slug = ?1",
            params![slug, goal],
        )
        .map_err(fail)?;
    }
    if let Some(words) = &facts.words {
        let words = words
            .iter()
            .map(|q| json!({"quote": clean_text(&q.quote, 1200), "source": q.source.as_deref().map(|s| clean_text(s, MAX_TITLE)), "date": q.date.as_deref().map(|d| clean_text(d, 40))}))
            .collect::<Vec<_>>();
        tx.execute(
            "UPDATE targets SET words = ?2 WHERE slug = ?1",
            params![slug, Value::from(words).to_string()],
        )
        .map_err(fail)?;
    }
    if let Some(sessions) = &facts.sessions {
        let lines = sessions
            .iter()
            .map(|s| clean_text(s, MAX_NOTE))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        tx.execute(
            "UPDATE targets SET sessions = ?2 WHERE slug = ?1",
            params![slug, json!(lines).to_string()],
        )
        .map_err(fail)?;
    }
    tx.execute(
        "UPDATE targets SET updated_ms = ?2 WHERE slug = ?1",
        params![slug, now],
    )
    .map_err(fail)?;
    Ok(())
}

fn need_target(tx: &Transaction<'_>, slug: &str) -> Result<(), String> {
    let exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM targets WHERE slug = ?1)",
            [slug],
            |r| r.get(0),
        )
        .map_err(fail)?;
    if exists {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO targets(slug, label, updated_ms) VALUES (?1, ?1, 0)",
        [slug],
    )
    .map_err(fail)?;
    Ok(())
}

fn scope_refs(
    tx: &Transaction<'_>,
    source: &Source<'_>,
    reference: &str,
    report: &mut Report,
) -> Result<Vec<String>, String> {
    let reference = reference.trim();
    if reference.ends_with('/') {
        return add_folder(tx, source, reference, report);
    }
    let lowered = reference.to_lowercase();
    if item_exists(tx, &lowered)? {
        return Ok(vec![lowered]);
    }
    if let Some(code) = file_code(tx, reference)? {
        return Ok(vec![code]);
    }
    if reference.contains('/') || reference.contains('.') && source.root.join(reference).is_file() {
        return Ok(vec![add_file(tx, source, reference, None, report)?]);
    }
    Err(format!(
        "{reference:?} is not an item id, a file on the map, a file path or a folder ending in /"
    ))
}

fn apply_scope(
    tx: &Transaction<'_>,
    source: &Source<'_>,
    slug: &str,
    rows: &[ScopeWrite],
    report: &mut Report,
) -> Result<(), String> {
    need_target(tx, slug)?;
    let mut position: i64 = tx
        .query_row(
            "SELECT coalesce(max(position), 0) FROM scope WHERE target = ?1",
            [slug],
            |r| r.get(0),
        )
        .map_err(fail)?;
    for row in rows {
        let refs = scope_refs(tx, source, &row.r#ref, report)?;
        for reference in refs {
            match row.state {
                ScopeState::In => {
                    position += 1;
                    tx.execute(
                        "INSERT INTO scope(target, ref, state, position) VALUES (?1, ?2, 'in', ?3)
                         ON CONFLICT(target, ref) DO UPDATE SET state = 'in', position = excluded.position, reason = NULL, confirmed = 0",
                        params![slug, reference, position],
                    )
                    .map_err(fail)?;
                }
                ScopeState::Cut => {
                    let reason = row
                        .reason
                        .as_deref()
                        .map(|r| clean_text(r, MAX_NOTE))
                        .filter(|r| !r.is_empty())
                        .ok_or("a cut needs reason: what is cut, by its content, and why")?;
                    position += 1;
                    tx.execute(
                        "INSERT INTO scope(target, ref, state, position, reason, confirmed) VALUES (?1, ?2, 'cut', ?3, ?4, ?5)
                         ON CONFLICT(target, ref) DO UPDATE SET state = 'cut', reason = excluded.reason, confirmed = excluded.confirmed",
                        params![slug, reference, position, reason, row.confirmed.unwrap_or(false) as i64],
                    )
                    .map_err(fail)?;
                }
                ScopeState::Confirm => {
                    let changed = tx
                        .execute(
                            "UPDATE scope SET confirmed = 1 WHERE target = ?1 AND ref = ?2 AND state = 'cut'",
                            params![slug, reference],
                        )
                        .map_err(fail)?;
                    if changed == 0 {
                        return Err(format!("{reference} has no cut to confirm for {slug}"));
                    }
                }
                ScopeState::Clear => {
                    tx.execute(
                        "DELETE FROM scope WHERE target = ?1 AND ref = ?2",
                        params![slug, reference],
                    )
                    .map_err(fail)?;
                }
            }
            report.scope_rows += 1;
        }
    }
    Ok(())
}

fn ids(tx: &Transaction<'_>, list: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for id in list {
        let id = need_item(tx, id)?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

fn valid_day(text: &str) -> Result<String, String> {
    let text = text.trim();
    chrono::NaiveDate::parse_from_str(text.get(..10).unwrap_or(""), "%Y-%m-%d")
        .map(|d| d.to_string())
        .map_err(|_| format!("done {text:?} must be a date as YYYY-MM-DD"))
}

fn set_blocks(tx: &Transaction<'_>, slug: &str, blocks: &[BlockWrite]) -> Result<(), String> {
    need_target(tx, slug)?;
    tx.execute("DELETE FROM blocks WHERE target = ?1", [slug])
        .map_err(fail)?;
    for (n, block) in blocks.iter().enumerate() {
        let title = clean_text(&block.title, MAX_TITLE);
        if title.is_empty() {
            return Err(format!("block {} needs a title", n + 1));
        }
        let opens = block.opens_with.as_deref().map(|o| clean_text(o, 40));
        if let Some(opens) = &opens
            && !matches!(
                opens.as_str(),
                "test" | "worked" | "question" | "teach" | "paper"
            )
        {
            return Err(format!(
                "opens_with {opens:?} is not one of test, worked, question, teach, paper"
            ));
        }
        let done = block.done.as_deref().map(valid_day).transpose()?;
        tx.execute(
            "INSERT INTO blocks(target, n, title, sections, tasks, minutes, opens_with, note, done, spent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                slug,
                (n + 1) as i64,
                title,
                json!(ids(tx, &block.sections)?).to_string(),
                json!(ids(tx, &block.tasks)?).to_string(),
                block.minutes,
                opens,
                block.note.as_deref().map(|n| clean_text(n, MAX_NOTE)),
                done,
                block.spent,
            ],
        )
        .map_err(fail)?;
    }
    Ok(())
}

fn set_carried(tx: &Transaction<'_>, slug: &str, carried: &[CarryWrite]) -> Result<(), String> {
    need_target(tx, slug)?;
    tx.execute("DELETE FROM carried WHERE target = ?1", [slug])
        .map_err(fail)?;
    for carry in carried {
        let idea = need_item(tx, &carry.idea)?;
        let by = need_item(tx, &carry.by)?;
        if idea == by {
            return Err(format!("{idea} cannot carry itself"));
        }
        let reserve = carry
            .reserve
            .as_deref()
            .map(|r| need_item(tx, r))
            .transpose()?;
        tx.execute(
            "INSERT INTO carried(target, idea, by_item, reserve) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(target, idea) DO UPDATE SET by_item = excluded.by_item, reserve = excluded.reserve",
            params![slug, idea, by, reserve],
        )
        .map_err(fail)?;
    }
    Ok(())
}

fn add_trace(tx: &Transaction<'_>, trace: &TraceWrite) -> Result<(), String> {
    let question = need_item(tx, &trace.question)?;
    let source = need_item(tx, &trace.source)?;
    let changed = clean_text(&trace.changed, MAX_NOTE);
    if changed.is_empty() {
        return Err(
            "a trace needs changed: what the test changed from its source, or \"nothing\"".into(),
        );
    }
    tx.execute(
        "INSERT INTO traces(question, source, changed) VALUES (?1, ?2, ?3)
         ON CONFLICT(question, source) DO UPDATE SET changed = excluded.changed",
        params![question, source, changed],
    )
    .map_err(fail)?;
    Ok(())
}

fn drop_missing_from_blocks(tx: &Transaction<'_>) -> Result<(), String> {
    let rows = {
        let mut statement = tx
            .prepare("SELECT target, n, sections, tasks FROM blocks")
            .map_err(fail)?;
        statement
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(fail)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(fail)?
    };
    for (target, n, sections, tasks) in rows {
        let keep = |text: &str| -> Result<String, String> {
            let mut kept = Vec::new();
            for id in serde_json::from_str::<Vec<String>>(text).unwrap_or_default() {
                if item_exists(tx, &id)? {
                    kept.push(id);
                }
            }
            Ok(json!(kept).to_string())
        };
        tx.execute(
            "UPDATE blocks SET sections = ?3, tasks = ?4 WHERE target = ?1 AND n = ?2",
            params![target, n, keep(&sections)?, keep(&tasks)?],
        )
        .map_err(fail)?;
    }
    Ok(())
}

pub(crate) fn apply(
    tx: &Transaction<'_>,
    source: &Source<'_>,
    request: &CourseMapRequest,
) -> Result<(Report, i64), String> {
    let mut report = Report::default();
    let slug = request.target.as_deref().map(valid_slug).transpose()?;
    if request.items.len() > MAX_ITEMS_PER_WRITE {
        return Err(format!("at most {MAX_ITEMS_PER_WRITE} items in one write"));
    }
    let needs_target = request.facts.is_some()
        || !request.scope.is_empty()
        || request.blocks.is_some()
        || !request.blocks_done.is_empty()
        || request.carried.is_some();
    if needs_target && slug.is_none() {
        return Err("facts, scope, blocks and carried need target".into());
    }
    if let (Some(slug), Some(facts)) = (&slug, &request.facts) {
        set_facts(tx, slug, facts, source.now_ms)?;
    }
    for file in &request.files {
        add_file(tx, source, &file.path, file.code.as_deref(), &mut report)?;
    }
    for (n, item) in request.items.iter().enumerate() {
        let done = match item.id.as_deref() {
            Some(id) => change_item(tx, id, item).map(|_| report.items_changed += 1),
            None => add_item(tx, source, item, &mut report).map(|_| ()),
        };
        done.map_err(|e| format!("items[{n}]: {e}"))?;
    }
    for id in &request.remove_items {
        let id = need_item(tx, id)?;
        let gone = {
            let mut statement = tx
                .prepare(
                    "WITH RECURSIVE under(id) AS (SELECT ?1 UNION SELECT items.id FROM items JOIN under ON items.parent = under.id)
                     SELECT id FROM under",
                )
                .map_err(fail)?;
            statement
                .query_map([&id], |r| r.get::<_, String>(0))
                .map_err(fail)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(fail)?
        };
        for removed in &gone {
            tx.execute("DELETE FROM scope WHERE ref = ?1", [removed])
                .map_err(fail)?;
            tx.execute("INSERT OR IGNORE INTO retired(id) VALUES (?1)", [removed])
                .map_err(fail)?;
        }
        report.items_removed += tx
            .execute("DELETE FROM items WHERE id = ?1", [&id])
            .map_err(fail)?;
    }
    if !request.remove_items.is_empty() {
        drop_missing_from_blocks(tx)?;
    }
    for (n, write) in request.links.iter().enumerate() {
        link(tx, write, true, &mut report).map_err(|e| format!("links[{n}]: {e}"))?;
    }
    for (n, write) in request.unlink.iter().enumerate() {
        link(tx, write, false, &mut report).map_err(|e| format!("unlink[{n}]: {e}"))?;
    }
    if let Some(slug) = &slug {
        if !request.scope.is_empty() {
            apply_scope(tx, source, slug, &request.scope, &mut report)?;
        }
        if let Some(blocks) = &request.blocks {
            set_blocks(tx, slug, blocks).map_err(|e| format!("blocks: {e}"))?;
            report.blocks = Some(blocks.len());
        }
        for done in &request.blocks_done {
            let day = valid_day(&done.done)?;
            let changed = tx
                .execute(
                    "UPDATE blocks SET done = ?3, spent = ?4 WHERE target = ?1 AND n = ?2",
                    params![slug, done.n, day, done.spent],
                )
                .map_err(fail)?;
            if changed == 0 {
                return Err(format!("blocks_done: {slug} has no block {}", done.n));
            }
        }
        if let Some(carried) = &request.carried {
            set_carried(tx, slug, carried).map_err(|e| format!("carried: {e}"))?;
            report.carried = Some(carried.len());
        }
        report.targets.insert(slug.clone());
    }
    for (n, trace) in request.traces.iter().enumerate() {
        add_trace(tx, trace).map_err(|e| format!("traces[{n}]: {e}"))?;
        report.traces += 1;
    }
    let revision = bump(tx)?;
    Ok((report, revision))
}
