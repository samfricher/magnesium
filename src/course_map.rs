mod facts;
mod render;
pub(crate) mod request;
mod store;
mod write;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use chrono::{Local, NaiveDate};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::workspace::{Workspace, reject_symlink_components, source_index_path};
use facts::{Counts, Evidence, Map, days_left, planned, reach_of};
pub(crate) use request::CourseMapRequest;
use request::MapAction;
use store::MapStore;
use write::{Source, valid_slug};

const LISTED: usize = 40;
const LISTED_FOLDERS: usize = 120;

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn now_ms() -> i64 {
    Local::now().timestamp_millis()
}

fn workspace_at(cwd: &Path) -> Result<Workspace, String> {
    Workspace::discover(cwd)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no Magnesium study folder here; run `magnesium init` first".to_owned())
}

fn sha(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn write_page(
    workspace: &Workspace,
    store: &MapStore,
    slug: &str,
    name: &str,
    text: &str,
) -> Result<String, String> {
    let relative = format!("Study/{slug}/{name}");
    let path = workspace.root().join(&relative);
    reject_symlink_components(&path, workspace.root()).map_err(|e| e.to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let current = match std::fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => std::fs::read_to_string(&path).ok(),
        Ok(_) => {
            return Err(format!(
                "{relative} is not a regular file, so Magnesium did not write it"
            ));
        }
        Err(_) => None,
    };
    if current.as_deref() == Some(text) {
        store.remember_page(&relative, &sha(text))?;
        return Ok(relative);
    }
    if let Some(current) = &current
        && store.page_hash(&relative)?.as_deref() != Some(sha(current).as_str())
    {
        let stamp = Local::now().format("%Y%m%dT%H%M%S%3f");
        let backup = workspace
            .control()
            .join("backups")
            .join("map")
            .join(format!(
                "{slug}-{}-{stamp}.md",
                name.trim_end_matches(".md")
            ));
        if let Some(parent) = backup.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::workspace::write_private_atomic(&backup, current.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    crate::workspace::write_user_owned_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())?;
    store.remember_page(&relative, &sha(text))?;
    Ok(relative)
}

fn write_pages(
    workspace: &Workspace,
    store: &MapStore,
    map: &Map,
    evidence: &Evidence,
    slugs: &[String],
) -> Result<Vec<String>, String> {
    let mut pages = Vec::new();
    for slug in slugs {
        let Some(target) = map.target(slug) else {
            continue;
        };
        pages.push(write_page(
            workspace,
            store,
            slug,
            "map.md",
            &render::map_page(map, evidence, target, today()),
        )?);
        pages.push(write_page(
            workspace,
            store,
            slug,
            "plan.md",
            &render::plan_page(map, evidence, target, today()),
        )?);
    }
    Ok(pages)
}

fn evidence_for(workspace: &Workspace) -> Evidence {
    Evidence::load(&workspace.store_path(), workspace.root(), today())
}

pub(crate) fn handle(cwd: &Path, request: CourseMapRequest) -> Result<Value, String> {
    let workspace = workspace_at(cwd)?;
    match request.action {
        MapAction::Inventory => inventory(&workspace, request.path_prefix.as_deref()),
        MapAction::AddFiles | MapAction::Write => {
            if request.action == MapAction::AddFiles && request.files.is_empty() {
                return Err("add_files needs files: [{\"path\": \"...\"}]".into());
            }
            let mut store = MapStore::open(&workspace)?;
            let index = source_index_path(workspace.root());
            let source = Source {
                root: workspace.root(),
                index: &index,
                now_ms: now_ms(),
            };
            let tx = store.write()?;
            let (report, revision) = write::apply(&tx, &source, &request)?;
            tx.commit().map_err(store::fail)?;
            let map = Map::load(&store.conn)?;
            let evidence = evidence_for(&workspace);
            let slugs = map
                .targets
                .iter()
                .map(|t| t.slug.clone())
                .collect::<Vec<_>>();
            let pages = write_pages(&workspace, &store, &map, &evidence, &slugs)?;
            let mut value = report.to_json(revision);
            if let Some(slug) = report.targets.iter().next() {
                value["coverage"] = target_counts(&map, &evidence, slug);
            }
            value["pages"] = json!(pages);
            Ok(value)
        }
        MapAction::Read => {
            let Some(store) = MapStore::open_existing(&workspace)? else {
                return Ok(
                    json!({"map": "none", "note": "No course map yet. A planning run builds it: inventory the folder, add the files in scope, then write items, links, scope and the schedule."}),
                );
            };
            let map = Map::load(&store.conn)?;
            let evidence = evidence_for(&workspace);
            if let Some(item) = request.item.as_deref() {
                return item_view(&map, &evidence, item, request.target.as_deref());
            }
            if let Some(file) = request.file.as_deref() {
                return file_view(&map, &evidence, file, &source_index_path(workspace.root()));
            }
            match request.target.as_deref() {
                Some(slug) => target_view(&workspace, &map, &evidence, &valid_slug(slug)?, false),
                None => Ok(overview(&map, &evidence)),
            }
        }
        MapAction::Reach => {
            let Some(store) = MapStore::open_existing(&workspace)? else {
                return Err("no course map yet".into());
            };
            let map = Map::load(&store.conn)?;
            let evidence = evidence_for(&workspace);
            reach(
                &map,
                &evidence,
                request.target.as_deref(),
                &request.candidates,
            )
        }
    }
}

fn target_counts(map: &Map, evidence: &Evidence, slug: &str) -> Value {
    let scoped = map.in_scope(slug);
    let plan = planned(map, slug);
    let mut counts = Counts::default();
    let mut no_task = 0;
    for id in scoped.items.iter().filter(|id| map.is_leaf(id)) {
        counts.add(evidence, id);
        if !plan.asked(id) {
            no_task += 1;
        }
    }
    let mut value = counts.to_json();
    value["asked_by_no_planned_task"] = json!(no_task);
    value["cut"] = json!(scoped.cut.iter().filter(|id| map.is_leaf(id)).count());
    value["unconfirmed_cuts"] = json!(scoped.cuts.iter().filter(|c| !c.confirmed).count());
    value
}

fn overview(map: &Map, evidence: &Evidence) -> Value {
    json!({
        "revision": map.revision,
        "files_on_map": map.files.len(),
        "items": map.items.len(),
        "links": map.uses.values().map(Vec::len).sum::<usize>(),
        "targets": map.targets.iter().map(|t| json!({
            "target": t.slug,
            "label": t.label,
            "at": t.at,
            "days_left": days_left(t.at.as_deref(), today()),
            "coverage": target_counts(map, evidence, &t.slug),
        })).collect::<Vec<_>>(),
    })
}

fn item_line(map: &Map, evidence: &Evidence, id: &str) -> Value {
    let item = map.item(id);
    json!({
        "id": id,
        "title": item.map(|i| i.title.clone()),
        "kind": item.map(|i| i.kind.clone()),
        "record": evidence.phrase(id),
    })
}

fn target_view(
    workspace: &Workspace,
    map: &Map,
    evidence: &Evidence,
    slug: &str,
    full: bool,
) -> Result<Value, String> {
    let Some(target) = map.target(slug) else {
        return Ok(
            json!({"target": slug, "map": "none", "note": format!("No target {slug} on the map yet. Write its facts and scope first.")}),
        );
    };
    let scoped = map.in_scope(slug);
    let plan = planned(map, slug);
    let leaves = scoped
        .items
        .iter()
        .filter(|id| map.is_leaf(id))
        .cloned()
        .collect::<Vec<_>>();
    let never = leaves
        .iter()
        .filter(|id| evidence.of(id).asked == 0)
        .cloned()
        .collect::<Vec<_>>();
    let no_task = leaves
        .iter()
        .filter(|id| !plan.asked(id))
        .cloned()
        .collect::<Vec<_>>();
    let files = render::coverage_by_top(map, evidence, &scoped)
        .into_iter()
        .map(|(group, counts)| {
            let mut value = counts.to_json();
            match map.file(&group) {
                Some(file) => {
                    value["file"] = json!(file.path);
                    value["code"] = json!(file.code);
                    if !file.readable {
                        value["read_by_eye"] = json!(true);
                    }
                }
                None => {
                    value["group"] = json!(group);
                    value["title"] = json!(map.title(&group));
                }
            }
            value
        })
        .collect::<Vec<_>>();
    let blocks = map.blocks.get(slug).cloned().unwrap_or_default();
    let schedule = blocks
        .iter()
        .map(|b| {
            json!({
                "n": b.n,
                "title": b.title,
                "opens_with": b.opens_with,
                "sections": b.sections,
                "tasks": b.tasks,
                "minutes": b.minutes,
                "done": b.done,
                "spent": b.spent,
            })
        })
        .collect::<Vec<_>>();
    let cuts = scoped
        .cuts
        .iter()
        .map(|c| json!({"ref": c.reference, "title": map.item(&c.reference).map(|i| i.title.clone()).or_else(|| map.file(&c.reference).map(|f| f.path.clone())), "reason": c.reason, "agreed": c.confirmed}))
        .collect::<Vec<_>>();
    let mut value = json!({
        "target": slug,
        "label": target.label,
        "at": target.at,
        "days_left": days_left(target.at.as_deref(), today()),
        "hours": target.hours,
        "goal": target.goal,
        "revision": map.revision,
        "coverage": target_counts(map, evidence, slug),
        "files": files,
        "schedule": schedule,
        "cuts": cuts,
        "pages": [format!("Study/{slug}/map.md"), format!("Study/{slug}/plan.md")],
    });
    let listed = if full { usize::MAX } else { LISTED };
    value["never_asked"] = json!(
        never
            .iter()
            .take(listed)
            .map(|id| item_line(map, evidence, id))
            .collect::<Vec<_>>()
    );
    value["asked_by_no_planned_task"] = json!(
        no_task
            .iter()
            .take(listed)
            .map(|id| item_line(map, evidence, id))
            .collect::<Vec<_>>()
    );
    if !full && (never.len() > LISTED || no_task.len() > LISTED) {
        value["note"] = json!(format!(
            "Lists show the first {LISTED} in the instructor's order; map.md has every item."
        ));
    }
    if full {
        value["items"] = json!(scoped.items.iter().map(|id| {
            let item = map.item(id).expect("scoped items are on the map");
            json!({
                "id": id,
                "title": item.title,
                "kind": item.kind,
                "origin": item.origin,
                "parent": item.parent,
                "file": item.file.as_deref().and_then(|f| map.file(f)).map(|f| f.path.clone()),
                "anchor_start": item.start,
                "anchor_end": item.end,
                "leaf": map.is_leaf(id),
                "record": evidence.phrase(id),
                "asked_by_plan": plan.asked(id),
                "uses": map.uses.get(id).cloned().unwrap_or_default(),
                "used_by": map.used_by.get(id).cloned().unwrap_or_default(),
                "reach": reach_of(map, evidence, id),
            })
        }).collect::<Vec<_>>());
        value["carried"] = json!(
            map.carried
                .get(slug)
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|c| json!({"idea": c.idea, "by": c.by, "reserve": c.reserve}))
                .collect::<Vec<_>>()
        );
        value["traces"] = json!(
            map.traces
                .iter()
                .map(|t| json!({"question": t.question, "source": t.source, "changed": t.changed}))
                .collect::<Vec<_>>()
        );
        let index = source_index_path(workspace.root());
        if let Ok(files) = crate::retrieval::indexed_files(&index) {
            let on_map = map
                .files
                .iter()
                .map(|f| f.path.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            value["folder"] = json!({"files": files.len(), "on_map": files.iter().filter(|f| on_map.contains(f.path.as_str())).count()});
        }
    }
    Ok(value)
}

fn file_view(
    map: &Map,
    evidence: &Evidence,
    reference: &str,
    index: &Path,
) -> Result<Value, String> {
    let file = map
        .file_by_ref(reference)
        .ok_or_else(|| format!("{reference:?} is not on the map; add_files puts it there"))?;
    let now = crate::retrieval::sections_at(index, Path::new(&file.path))
        .ok()
        .flatten()
        .and_then(|(row, _)| row.content_sha256);
    let items = map
        .items
        .iter()
        .filter(|i| i.file.as_deref() == Some(file.code.as_str()))
        .map(|i| {
            let mut value = json!({"id": i.id, "title": i.title, "kind": i.kind});
            if now.is_some() && i.sha.is_some() && i.sha != now {
                value["changed_since_taken"] = json!(true);
            }
            if let Some(parent) = &i.parent {
                value["parent"] = json!(parent);
            }
            if let Some(start) = i.start {
                value["anchor_start"] = json!(start);
                value["anchor_end"] = json!(i.end);
            }
            if map.is_leaf(&i.id) {
                value["record"] = json!(evidence.phrase(&i.id));
            }
            if let Some(uses) = map.uses.get(&i.id) {
                value["uses"] = json!(uses);
            }
            if let Some(by) = map.used_by.get(&i.id) {
                value["used_by"] = json!(by);
            }
            value
        })
        .collect::<Vec<_>>();
    let changed = matches!((&file.sha, &now), (Some(mapped), Some(now)) if mapped != now);
    Ok(json!({
        "file": file.path,
        "code": file.code,
        "readable": file.readable,
        "anchor": if file.path.to_lowercase().ends_with(".pdf") { "page" } else { "line" },
        "items": items,
        "changed_since_mapped": changed,
    }))
}

fn item_view(
    map: &Map,
    evidence: &Evidence,
    id: &str,
    slug: Option<&str>,
) -> Result<Value, String> {
    let id = id.trim().to_lowercase();
    let item = map
        .item(&id)
        .ok_or_else(|| format!("no item {id:?} on the map"))?;
    let mut value = json!({
        "id": item.id,
        "title": item.title,
        "kind": item.kind,
        "origin": item.origin,
        "file": item.file.as_deref().and_then(|f| map.file(f)).map(|f| f.path.clone()),
        "anchor_start": item.start,
        "anchor_end": item.end,
        "parent": item.parent,
        "children": map.children.get(&id).cloned().unwrap_or_default(),
        "note": item.note,
        "uses": map.uses.get(&id).cloned().unwrap_or_default().iter().map(|u| item_line(map, evidence, u)).collect::<Vec<_>>(),
        "used_by": map.used_by.get(&id).cloned().unwrap_or_default().iter().map(|u| item_line(map, evidence, u)).collect::<Vec<_>>(),
        "record": evidence.phrase(&id),
        "reach": reach_of(map, evidence, &id),
    });
    if let Some(slug) = slug {
        let plan = planned(map, slug);
        if let Some(tasks) = plan.asked_by.get(&id) {
            value["asked_by"] = json!(tasks);
        }
        if let Some((by, reserve)) = plan.carried_by.get(&id) {
            value["carried_by"] = json!({"by": by, "reserve": reserve});
        }
    }
    let traced = map
        .traces
        .iter()
        .filter(|t| t.question == id || t.source == id)
        .map(|t| json!({"question": t.question, "source": t.source, "changed": t.changed}))
        .collect::<Vec<_>>();
    if !traced.is_empty() {
        value["traces"] = json!(traced);
    }
    Ok(value)
}

fn reach(
    map: &Map,
    evidence: &Evidence,
    slug: Option<&str>,
    candidates: &[String],
) -> Result<Value, String> {
    let mut ids: Vec<String> = candidates.iter().map(|c| c.trim().to_lowercase()).collect();
    if ids.is_empty() {
        let slug = slug.ok_or("reach needs candidates or target")?;
        let slug = valid_slug(slug)?;
        let scoped = map.in_scope(&slug);
        ids.extend(
            scoped
                .items
                .iter()
                .filter(|id| {
                    map.item(id)
                        .is_some_and(|i| i.parent.is_none() || !map.is_leaf(id))
                })
                .cloned(),
        );
        for block in map.blocks.get(&slug).into_iter().flatten() {
            for task in &block.tasks {
                if !ids.contains(task) {
                    ids.push(task.clone());
                }
            }
        }
    }
    for id in &ids {
        if map.item(id).is_none() {
            return Err(format!("no item {id:?} on the map"));
        }
    }
    let mut rows = ids
        .iter()
        .map(|id| reach_of(map, evidence, id))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        let key = |v: &Value| {
            (
                v["never_asked"].as_u64().unwrap_or(0)
                    + v["no_blank_page_pass"].as_u64().unwrap_or(0)
                    + v["due"].as_u64().unwrap_or(0),
                v["uses"].as_u64().unwrap_or(0),
            )
        };
        key(b).cmp(&key(a))
    });
    Ok(json!({
        "reach": rows,
        "note": "For each candidate, the items it uses (its use links and the items inside it): how many were never asked, have no blank-page pass, or are due. Counts only, sorted by their sum; nothing is weighted and nothing passes along a link.",
    }))
}

fn kind_of(file: &crate::retrieval::IndexedFile) -> &'static str {
    let extension = Path::new(&file.path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match (file.source_kind.as_str(), file.exclusion_reason.as_deref()) {
        (_, Some("sensitive" | "sensitive_content" | "sensitive_extracted")) => "private",
        ("pdf", _) => "pdf",
        ("text", _) => "text",
        _ if [
            "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp",
        ]
        .contains(&extension.as_str()) =>
        {
            "image"
        }
        _ if [
            "doc", "docx", "ppt", "pptx", "key", "pages", "xls", "xlsx", "numbers", "odt", "odp",
            "ods", "rtf", "epub", "djvu",
        ]
        .contains(&extension.as_str()) =>
        {
            "document"
        }
        _ => "other",
    }
}

fn page_code(path: &str) -> bool {
    let extension = Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    ["css", "scss", "less", "js", "mjs", "cjs", "map"].contains(&extension.as_str())
        || path.split('/').rev().skip(1).any(|part| {
            ["_assets", "site_libs", "libs", "assets", "static", "vendor"].contains(&part)
                || part.ends_with("_files")
        })
}

fn inventory(workspace: &Workspace, prefix: Option<&str>) -> Result<Value, String> {
    let index = source_index_path(workspace.root());
    if !index.is_file() {
        return Ok(
            json!({"indexed": false, "note": "The folder has not been indexed yet. Ask again in a moment."}),
        );
    }
    let files = crate::retrieval::indexed_files(&index).map_err(|e| e.to_string())?;
    let status = crate::retrieval::folder_files(&index).ok();
    let on_map: BTreeMap<String, (String, Option<String>)> =
        match MapStore::open_existing(workspace)? {
            Some(store) => Map::load(&store.conn)?
                .files
                .into_iter()
                .map(|f| (f.path, (f.code, f.sha)))
                .collect(),
            None => BTreeMap::new(),
        };
    let mut changed = Vec::new();
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut folders: BTreeMap<String, [usize; 5]> = BTreeMap::new();
    let mut listing = Vec::new();
    let mut page_files = 0;
    for file in &files {
        if file.exclusion_reason.as_deref() == Some("magnesiumignore") {
            continue;
        }
        if page_code(&file.path) {
            page_files += 1;
            continue;
        }
        let kind = kind_of(file);
        *kinds.entry(kind).or_insert(0) += 1;
        if kind == "private" {
            continue;
        }
        let readable =
            matches!(kind, "text" | "pdf") && file.index_status == "indexed" && file.has_text;
        let eye = !readable || (kind == "pdf" && file.titled == 0);
        let folder = Path::new(&file.path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| ".".to_owned());
        let counts = folders.entry(folder).or_default();
        counts[0] += 1;
        counts[1] += readable as usize;
        counts[2] += (file.titled > 0) as usize;
        counts[3] += eye as usize;
        counts[4] += on_map.contains_key(&file.path) as usize;
        if let Some(prefix) = prefix
            && file.path.starts_with(prefix.trim_start_matches("./"))
        {
            let mut row = json!({"path": file.path, "kind": kind});
            if readable {
                row["sections"] = json!(file.titled);
            } else if file.index_status != "indexed" && matches!(kind, "text" | "pdf") {
                row["waiting"] = json!(true);
            }
            if eye {
                row["read_by_eye"] = json!(true);
            }
            if let Some((code, _)) = on_map.get(&file.path) {
                row["on_map"] = json!(code);
            }
            listing.push(row);
        }
        if let Some((code, Some(mapped))) = on_map.get(&file.path)
            && file
                .content_sha256
                .as_ref()
                .is_some_and(|now| now != mapped)
        {
            changed.push(json!({"path": file.path, "code": code}));
        }
    }
    let total_folders = folders.len();
    let mut value = json!({
        "files": kinds,
        "page_code_files": page_files,
        "folders": folders.into_iter().take(LISTED_FOLDERS).map(|(folder, c)| json!({
            "folder": folder, "files": c[0], "readable": c[1], "with_titles": c[2], "read_by_eye": c[3], "on_map": c[4],
        })).collect::<Vec<_>>(),
        "note": "read_by_eye counts images, documents, scans and PDFs with no titles: the runtime cannot name their sections, so the planning run reads them and adds their items. Files left out for privacy are counted under private and never listed.",
    });
    if total_folders > LISTED_FOLDERS {
        value["folders_not_listed"] = json!(total_folders - LISTED_FOLDERS);
    }
    if let Some(status) = status {
        value["indexed"] = json!(format!(
            "{} of {} readable files",
            status.indexed, status.readable
        ));
        value["first_scan_done"] = json!(status.first_scan_done);
    }
    if prefix.is_some() {
        value["listing"] = json!(listing);
    }
    if !changed.is_empty() {
        value["changed_since_mapped"] = json!(changed);
        value["note_changed"] = json!(
            "These files changed after their items were taken: read them again and revise their items."
        );
    }
    Ok(value)
}

pub(crate) fn notice_clause(workspace: &Workspace) -> Option<String> {
    let store = MapStore::open_existing(workspace).ok()??;
    let map = Map::load(&store.conn).ok()?;
    let today = today();
    let target = map
        .targets
        .iter()
        .filter(|t| map.scope.contains_key(&t.slug))
        .min_by_key(|t| match days_left(t.at.as_deref(), today) {
            Some(days) if days >= 0 => days,
            Some(_) => i64::MAX - 1,
            None => i64::MAX,
        })?;
    let evidence = evidence_for(workspace);
    let counts = target_counts(&map, &evidence, &target.slug);
    Some(format!(
        " Course map, {}: {} items in scope, {} never asked, {} asked by no planned task, {} due.",
        target.slug,
        counts["items"],
        counts["never_asked"],
        counts["asked_by_no_planned_task"],
        counts["due"]
    ))
}

pub(crate) fn scoped_ids(root: &Path, slug: &str) -> std::collections::BTreeSet<String> {
    let Ok(Some(workspace)) = Workspace::discover(root) else {
        return Default::default();
    };
    let Ok(Some(store)) = MapStore::open_existing(&workspace) else {
        return Default::default();
    };
    let Ok(map) = Map::load(&store.conn) else {
        return Default::default();
    };
    map.in_scope(slug).items.into_iter().collect()
}

pub(crate) fn item_known(workspace_root: &Path, id: &str) -> bool {
    let Ok(Some(workspace)) = Workspace::discover(workspace_root) else {
        return false;
    };
    let Ok(Some(store)) = MapStore::open_existing(&workspace) else {
        return false;
    };
    store
        .conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id = ?1)",
            [id.trim().to_lowercase()],
            |r| r.get::<_, bool>(0),
        )
        .unwrap_or(false)
}

pub(crate) fn print_map(target: Option<&str>, as_json: bool, rewrite: bool) -> ExitCode {
    let result = (|| -> Result<Value, String> {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        let workspace = workspace_at(&cwd)?;
        let Some(store) = MapStore::open_existing(&workspace)? else {
            return Ok(json!({"map": "none"}));
        };
        let map = Map::load(&store.conn)?;
        let evidence = evidence_for(&workspace);
        if rewrite {
            let slugs = map
                .targets
                .iter()
                .map(|t| t.slug.clone())
                .collect::<Vec<_>>();
            write_pages(&workspace, &store, &map, &evidence, &slugs)?;
        }
        match target {
            Some(slug) => target_view(&workspace, &map, &evidence, &valid_slug(slug)?, true),
            None => Ok(overview(&map, &evidence)),
        }
    })();
    match result {
        Ok(value) if as_json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&value).unwrap_or_default()
            );
            ExitCode::SUCCESS
        }
        Ok(value) => {
            print!("{}", text_view(&value));
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            ExitCode::FAILURE
        }
    }
}

fn text_view(value: &Value) -> String {
    if value["map"] == "none" {
        return "No course map in this folder yet.\n".into();
    }
    let mut text = String::new();
    if let Some(targets) = value["targets"].as_array() {
        text.push_str(&format!(
            "Course map: {} files, {} items, {} use links.\n",
            value["files_on_map"], value["items"], value["links"]
        ));
        for target in targets {
            let c = &target["coverage"];
            text.push_str(&format!(
                "{}: {} items in scope, {} never asked, {} asked by no planned task.\n",
                target["label"].as_str().unwrap_or(""),
                c["items"],
                c["never_asked"],
                c["asked_by_no_planned_task"]
            ));
        }
        return text;
    }
    let c = &value["coverage"];
    text.push_str(&format!(
        "{}: {} items in scope, {} never asked, {} asked by no planned task, {} cut.\n",
        value["label"].as_str().unwrap_or(""),
        c["items"],
        c["never_asked"],
        c["asked_by_no_planned_task"],
        c["cut"]
    ));
    for file in value["files"].as_array().into_iter().flatten() {
        text.push_str(&format!(
            "  {}: {} items, {} never asked\n",
            file["file"]
                .as_str()
                .or(file["title"].as_str())
                .unwrap_or(""),
            file["items"],
            file["never_asked"]
        ));
    }
    text
}
