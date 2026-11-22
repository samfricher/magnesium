use std::collections::BTreeMap;
use std::fmt::Write as _;

use chrono::{Datelike, NaiveDate};

use super::facts::{Counts, Evidence, Map, Scoped, Target, days_left, planned};

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn ids_text(map: &Map, ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("`{id}` {}", map.title(id)))
        .collect::<Vec<_>>()
        .join("; ")
}

fn when(target: &Target) -> String {
    let Some(at) = target.at.as_deref() else {
        return "no date".to_owned();
    };
    match NaiveDate::parse_from_str(at.get(..10).unwrap_or(""), "%Y-%m-%d") {
        Ok(date) => {
            let rest = at.get(10..).unwrap_or("").trim();
            let day = date.weekday().to_string();
            if rest.is_empty() {
                format!("{date} ({day})")
            } else {
                format!("{date} ({day}) {rest}")
            }
        }
        Err(_) => at.to_owned(),
    }
}

pub(crate) fn coverage_by_top(
    map: &Map,
    evidence: &Evidence,
    scoped: &Scoped,
) -> Vec<(String, Counts)> {
    let mut order: Vec<String> = Vec::new();
    let mut counts: BTreeMap<String, Counts> = BTreeMap::new();
    for id in scoped.items.iter().filter(|id| map.is_leaf(id)) {
        let item = map.item(id).expect("scoped items are on the map");
        let group = item.file.clone().unwrap_or_else(|| map.top(id));
        if !counts.contains_key(&group) {
            order.push(group.clone());
        }
        counts.entry(group).or_default().add(evidence, id);
    }
    order
        .into_iter()
        .map(|group| {
            let value = counts[&group];
            (group, value)
        })
        .collect()
}

fn group_name(map: &Map, group: &str) -> String {
    match map.file(group) {
        Some(file) => format!("`{}` ({})", file.path, file.code),
        None => format!("`{group}` {}", map.title(group)),
    }
}

pub(crate) fn map_page(
    map: &Map,
    evidence: &Evidence,
    target: &Target,
    today: NaiveDate,
) -> String {
    let scoped = map.in_scope(&target.slug);
    let mut total = Counts::default();
    for id in scoped.items.iter().filter(|id| map.is_leaf(id)) {
        total.add(evidence, id);
    }
    let plan = planned(map, &target.slug);
    let mut text = String::new();
    let _ = writeln!(text, "# Course map: {}\n", target.label);
    let _ = writeln!(
        text,
        "Magnesium writes this page from the course map (revision {}). The planning run changes the map; an edit made here is saved under `.magnesium/backups/map/` and replaced.\n",
        map.revision
    );
    let _ = writeln!(
        text,
        "In scope: {} items. Never asked: {}. No blank-page pass: {}. Due: {}. Cut: {}. Asked by no planned task: {}.\n",
        total.items,
        total.never_asked,
        total.no_pass,
        total.due,
        scoped.cut.iter().filter(|id| map.is_leaf(id)).count(),
        scoped
            .items
            .iter()
            .filter(|id| map.is_leaf(id) && !plan.asked(id))
            .count()
    );
    if let Some(days) = days_left(target.at.as_deref(), today) {
        let _ = writeln!(
            text,
            "{} on {}: {days} days from {today}.\n",
            target.label,
            when(target)
        );
    }
    let mut current_group: Option<String> = None;
    for id in &scoped.items {
        let item = map.item(id).expect("scoped items are on the map");
        let group = item.file.clone().unwrap_or_else(|| "outline".to_owned());
        if current_group.as_deref() != Some(group.as_str()) {
            match map.file(&group) {
                Some(file) => {
                    let _ = writeln!(text, "\n## {} (`{}`)\n", file.path, file.code);
                    if !file.readable {
                        let _ =
                            writeln!(text, "Not readable as text: its items were read by eye.\n");
                    }
                }
                None => {
                    let _ = writeln!(text, "\n## Not from a file\n");
                }
            }
            current_group = Some(group);
        }
        let depth = item.parent.as_ref().map_or(0, |_| {
            let mut depth = 0;
            let mut at = item.parent.clone();
            while let Some(parent) = at {
                depth += 1;
                at = map.item(&parent).and_then(|p| p.parent.clone());
                if depth > 12 {
                    break;
                }
            }
            depth
        });
        let indent = "  ".repeat(depth);
        let mut line = format!("{indent}- `{}` {}", item.id, map.title(&item.id));
        if item.kind != "section" {
            let _ = write!(line, " ({})", item.kind);
        }
        if let (Some(start), Some(end)) = (item.start, item.end) {
            let unit = map
                .file(item.file.as_deref().unwrap_or(""))
                .map(|f| {
                    if f.path.to_lowercase().ends_with(".pdf") {
                        "pages"
                    } else {
                        "lines"
                    }
                })
                .unwrap_or("lines");
            if start == end {
                let _ = write!(line, ", {} {start}", unit.trim_end_matches('s'));
            } else {
                let _ = write!(line, ", {unit} {start} to {end}");
            }
        }
        if map.is_leaf(id) {
            let _ = write!(line, ". {}", evidence.phrase(id));
        }
        if let Some(uses) = map.uses.get(id) {
            let _ = write!(
                line,
                ". Uses {}",
                uses.iter()
                    .map(|u| format!("`{u}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if let Some(by) = map.used_by.get(id) {
            let _ = write!(
                line,
                ". Used by {}",
                by.iter()
                    .map(|u| format!("`{u}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if let Some(note) = &item.note {
            let _ = write!(line, ". {note}");
        }
        let _ = writeln!(text, "{line}");
    }
    if !scoped.cuts.is_empty() {
        let _ = writeln!(text, "\n## Cut from this test\n");
        for row in &scoped.cuts {
            let what = map
                .item(&row.reference)
                .map(|i| format!("`{}` {}", i.id, i.title))
                .or_else(|| map.file(&row.reference).map(|f| format!("`{}`", f.path)))
                .unwrap_or_else(|| row.reference.clone());
            let _ = writeln!(
                text,
                "- {what}: {}. {}",
                row.reason.as_deref().unwrap_or(""),
                if row.confirmed {
                    "Agreed with you."
                } else {
                    "Not yet agreed with you."
                }
            );
        }
    }
    let in_scope_files = map
        .scope
        .get(&target.slug)
        .into_iter()
        .flatten()
        .filter(|r| r.state == "in")
        .filter_map(|r| map.file(&r.reference).map(|f| f.code.clone()))
        .collect::<Vec<_>>();
    let other = map
        .files
        .iter()
        .filter(|f| {
            !in_scope_files.contains(&f.code)
                && !map.items.iter().any(|i| {
                    i.file.as_deref() == Some(f.code.as_str()) && scoped.items.contains(&i.id)
                })
        })
        .collect::<Vec<_>>();
    if !other.is_empty() {
        let _ = writeln!(text, "\n## On the map, not in this scope\n");
        for file in other {
            let _ = writeln!(text, "- `{}` ({})", file.path, file.code);
        }
    }
    text
}

pub(crate) fn plan_page(
    map: &Map,
    evidence: &Evidence,
    target: &Target,
    today: NaiveDate,
) -> String {
    let scoped = map.in_scope(&target.slug);
    let plan = planned(map, &target.slug);
    let mut total = Counts::default();
    for id in scoped.items.iter().filter(|id| map.is_leaf(id)) {
        total.add(evidence, id);
    }
    let mut text = String::new();
    let _ = writeln!(text, "# {}: study plan\n", target.label);
    let _ = writeln!(
        text,
        "Magnesium writes this page from the course map (revision {}): `map.md` beside it lists every item. The planning run changes the plan; an edit made here is saved under `.magnesium/backups/map/` and replaced.\n",
        map.revision
    );
    let _ = writeln!(text, "## Facts\n\n| Item | Value |\n|---|---|");
    let _ = writeln!(text, "| Target | {} |", cell(&target.label));
    let _ = writeln!(text, "| When | {} |", cell(&when(target)));
    if let Some(days) = days_left(target.at.as_deref(), today) {
        let _ = writeln!(text, "| Days left | {days} from {today} |");
    }
    if let Some(minutes) = target.length_minutes {
        let _ = writeln!(text, "| Length | {minutes} minutes |");
    }
    if let Some(aids) = &target.aids {
        let _ = writeln!(text, "| Aids | {} |", cell(aids));
    }
    if let Some(hours) = target.hours {
        let _ = writeln!(text, "| Hours available | {hours} |");
    }
    if let Some(goal) = &target.goal {
        let _ = writeln!(text, "| Goal | {} |", cell(goal));
    }
    let _ = writeln!(
        text,
        "| Coverage | {} items in scope; {} never asked; {} with no blank-page pass; {} cut |",
        total.items,
        total.never_asked,
        total.no_pass,
        scoped.cut.iter().filter(|id| map.is_leaf(id)).count()
    );
    let words = target.words.as_array().cloned().unwrap_or_default();
    let _ = writeln!(text, "\n## The instructor's words\n");
    if words.is_empty() {
        let _ = writeln!(text, "- none found");
    }
    for quote in words {
        let mut line = format!("- \"{}\"", quote["quote"].as_str().unwrap_or(""));
        let source = [quote["source"].as_str(), quote["date"].as_str()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
        if !source.is_empty() {
            let _ = write!(line, " ({source})");
        }
        let _ = writeln!(text, "{line}");
    }
    let _ = writeln!(text, "\n## Sessions\n");
    if target.sessions.is_empty() {
        let _ = writeln!(text, "- none planned");
    }
    for line in &target.sessions {
        let _ = writeln!(text, "- {line}");
    }
    let blocks = map.blocks.get(&target.slug).cloned().unwrap_or_default();
    let _ = writeln!(
        text,
        "\n## Blocks\n\n| # | Block | Opens with | Sections | Tasks | Minutes | Status | Running total |\n|---|---|---|---|---|---|---|---|"
    );
    let mut running = 0i64;
    for block in &blocks {
        if block.done.is_none() {
            running += block.minutes.unwrap_or(0);
        }
        let status = match (&block.done, block.spent) {
            (Some(done), Some(spent)) => format!("done {done}, {spent} min"),
            (Some(done), None) => format!("done {done}"),
            (None, _) => "to do".to_owned(),
        };
        let mut title = block.title.clone();
        if let Some(note) = &block.note {
            let _ = write!(title, ". {note}");
        }
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {} | {:.1} h |",
            block.n,
            cell(&title),
            block.opens_with.as_deref().unwrap_or(""),
            cell(&ids_text(map, &block.sections)),
            cell(&ids_text(map, &block.tasks)),
            block.minutes.map(|m| m.to_string()).unwrap_or_default(),
            status,
            running as f64 / 60.0
        );
    }
    if blocks.is_empty() {
        let _ = writeln!(text, "| 1 | no blocks yet | | | | | | |");
    }
    let tasks = blocks
        .iter()
        .flat_map(|b| b.tasks.iter().cloned())
        .collect::<Vec<_>>();
    if !tasks.is_empty() {
        let _ = writeln!(text, "\n## Tasks\n");
        for task in &tasks {
            let Some(item) = map.item(task) else { continue };
            let mut line = format!("- `{}` {}", item.id, map.title(&item.id));
            if let Some(file) = item.file.as_deref().and_then(|f| map.file(f)) {
                let _ = write!(line, " (`{}`", file.path);
                if let Some(start) = item.start {
                    let _ = write!(line, " at {start}");
                }
                line.push(')');
            }
            if let Some(uses) = map.uses.get(task) {
                let _ = write!(
                    line,
                    ". Uses {}",
                    uses.iter()
                        .map(|u| format!("`{u}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            let _ = writeln!(text, "{line}");
        }
    }
    let _ = writeln!(
        text,
        "\n## Every item in scope\n\n| Item | What it is | Section | Asked by | Record |\n|---|---|---|---|---|"
    );
    for id in scoped.items.iter().filter(|id| map.is_leaf(id)) {
        let item = map.item(id).expect("scoped items are on the map");
        let section = map.top(id);
        let section = if section == *id {
            item.file.clone().unwrap_or_default()
        } else {
            format!("`{section}` {}", map.title(&section))
        };
        let asked = match (plan.asked_by.get(id), plan.carried_by.get(id)) {
            (_, Some((by, reserve))) => {
                let mut line = format!("carried by `{by}` {}", map.title(by));
                if let Some(reserve) = reserve {
                    let _ = write!(line, "; `{reserve}` if it breaks");
                }
                line
            }
            (Some(tasks), None) => tasks.join(", "),
            (None, None) => match plan.in_block.get(id) {
                Some(n) => format!("in block {n}, by no task"),
                None => "no task yet".to_owned(),
            },
        };
        let _ = writeln!(
            text,
            "| `{}` | {} | {} | {} | {} |",
            item.id,
            cell(&map.title(&item.id)),
            cell(&section),
            cell(&asked),
            evidence.phrase(id)
        );
    }
    if !scoped.cuts.is_empty() {
        let _ = writeln!(text, "\n## Cut from this test\n");
        for row in &scoped.cuts {
            let what = map
                .item(&row.reference)
                .map(|i| format!("`{}` {}", i.id, i.title))
                .or_else(|| map.file(&row.reference).map(|f| format!("`{}`", f.path)))
                .unwrap_or_else(|| row.reference.clone());
            let _ = writeln!(
                text,
                "- {what}: {}. {}",
                row.reason.as_deref().unwrap_or(""),
                if row.confirmed {
                    "Agreed with you."
                } else {
                    "Not yet agreed with you."
                }
            );
        }
    }
    let _ = writeln!(
        text,
        "\n## Coverage\n\n| Section | Items | Never asked | No blank-page pass | Due |\n|---|---|---|---|---|"
    );
    for (group, counts) in coverage_by_top(map, evidence, &scoped) {
        let _ = writeln!(
            text,
            "| {} | {} | {} | {} | {} |",
            cell(&group_name(map, &group)),
            counts.items,
            counts.never_asked,
            counts.no_pass,
            counts.due
        );
    }
    let traced = map
        .traces
        .iter()
        .filter(|t| scoped.items.contains(&t.source) || scoped.items.contains(&t.question))
        .collect::<Vec<_>>();
    if !traced.is_empty() {
        let _ = writeln!(
            text,
            "\n## Earlier tests, traced to the material\n\n| Question | Comes from | What changed |\n|---|---|---|"
        );
        for trace in traced {
            let _ = writeln!(
                text,
                "| `{}` {} | `{}` {} | {} |",
                trace.question,
                cell(&map.title(&trace.question)),
                trace.source,
                cell(&map.title(&trace.source)),
                cell(&trace.changed)
            );
        }
    }
    text
}
