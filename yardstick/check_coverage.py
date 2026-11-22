from __future__ import annotations

import shutil
from pathlib import Path

from common import Fixture, mentions, table


def own_plan_text(folder: Path | None) -> str:
    if not folder:
        return ""
    parts = []
    for path in sorted(folder.iterdir()):
        if path.is_file() and path.suffix in (".md", ".yaml", ".yml") and "." not in path.stem:
            parts.append(path.read_text(encoding="utf-8", errors="replace"))
    return "\n".join(parts)


def in_material(fixture: Fixture, refs: list[dict], terms: list[str]) -> tuple[str, list[str]]:
    seen, missing = [], []
    for ref in refs:
        path = fixture.folder / ref["path"]
        if not path.is_file():
            missing.append(ref["path"])
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            seen.append(f"{ref['path']} (not text, present)")
            continue
        where = ref.get("where")
        if (where and where.lower() in text.lower()) or any(mentions(text, t) for t in terms) or not (where or terms):
            seen.append(f"{ref['path']} {where or ''}".strip())
        else:
            missing.append(f"{ref['path']} {where or ''}".strip())
    if not refs:
        return "no material named", []
    if not missing:
        return "yes", seen
    return ("partly" if seen else "no"), seen + [f"missing: {m}" for m in missing]


def prepare_map(args, adapter, fixture: Fixture, place) -> str:
    source = getattr(args, "map_from", None)
    if source:
        store = Path(source) / ".magnesium" / "map.sqlite"
        if not store.is_file():
            raise SystemExit(f"No course map at {store}")
        shutil.copy2(store, place.folder / ".magnesium" / "map.sqlite")
        adapter.cli(place, "map", "--write-pages")
        return f"the map a planning run built, copied from {source}"
    scope = fixture.data.get("map_scope")
    if not scope:
        return "none: the fixture names no map_scope and no --map-from was given"
    settled = adapter.settle_index(place)
    adapter.map_scope(place, fixture.target, scope, fixture.test_label)
    return (f"the runtime alone: sections from headings for the fixture's map_scope ({len(scope)} entries), no model; "
            f"index settled in {settled.get('seconds')} s")


def reach_lines(adapter, place, fixture: Fixture, flags: dict) -> list[str]:
    view = adapter.map_view(place, fixture.target)
    if not view:
        return []
    items = {i["id"]: i for i in view["items"]}
    files = []
    for flag in flags.values():
        for item_id in flag["items"]:
            path = items[item_id].get("file")
            if path and path not in files:
                files.append(path)
    lines = []
    for path in files:
        tops = [i for i in view["items"] if i.get("file") == path and not i.get("parent")]
        ranked = sorted(tops, key=lambda i: -((i["reach"]["never_asked"] or 0) + (i["reach"]["no_blank_page_pass"] or 0) + (i["reach"]["due"] or 0)))
        if not ranked:
            continue
        lines.append(f"Reach among the top-level sections of {path} ({len(tops)} sections):")
        for item in ranked[:6]:
            used = [items[u]["title"] if u in items else u for u in item.get("uses") or []]
            lines.append(f"  {item['id']} {item['title']}: uses {item['reach']['uses']}, never asked {item['reach']['never_asked']}"
                         + (f"; uses {', '.join(used[:8])}" if used else ""))
    return lines


def run(args, adapter) -> tuple[dict, str]:
    fixture = Fixture(args.fixture)
    place = adapter.place(fixture.folder, f"coverage-{fixture.name}")
    built = prepare_map(args, adapter, fixture, place)
    probe = adapter.course_map(place, fixture.target)
    flags = adapter.flagged_untaught(place, fixture.target, fixture.asked().get("items", []))
    links = adapter.use_links(place, fixture.target)
    plan_folder = place.folder / fixture.target if fixture.target and (place.folder / fixture.target / "plan.md").is_file() and getattr(args, "map_from", None) else fixture.plan_folder()
    plan = own_plan_text(plan_folder)
    rows, items = [], []
    for item in fixture.asked().get("items", []):
        material, where = in_material(fixture, item.get("material", []), item.get("terms", []))
        wanted = item.get("plan_refs") or item.get("terms", [])
        refs = [r for r in wanted if plan and mentions(plan, r)]
        listed = "yes" if refs else ("no plan" if not plan else "no")
        taught = item.get("taught_before_test")
        flag = (flags or {}).get(item["id"])
        on_map = "no map" if flags is None else (f"yes, {flag['on_map']} items" if flag and flag["on_map"] else "no")
        planned = "no map" if flags is None else (f"{flag['asked_by_plan']} of {flag['leaves']}" if flag and flag["leaves"] else "-")
        was_flagged = None if flags is None else bool(flag and flag["flagged"])
        items.append({"id": item["id"], "label": item.get("label"), "in_materials": material, "where": where,
                      "plan_listed": listed, "plan_matches": refs, "taught_before_test": taught, "map": flag,
                      "runtime_flagged_untaught": was_flagged, "evidence": item.get("evidence")})
        rows.append([item["id"], item.get("label"), material, on_map, planned,
                     {True: "yes", False: "NO", None: "unknown"}[taught],
                     "no (no map)" if was_flagged is None else ("yes" if was_flagged else "no")])
    untaught = [i for i in items if i["taught_before_test"] is False]
    flagged_count = sum(1 for i in untaught if i["runtime_flagged_untaught"])
    covered = sum(1 for i in untaught if i["map"] and i["map"]["leaves"] and i["map"]["asked_by_plan"] == i["map"]["leaves"])
    silent = sum(1 for i in untaught if not i["map"] or not i["map"]["on_map"]
                 or (not i["runtime_flagged_untaught"] and i["map"]["asked_by_plan"] < i["map"]["leaves"]))
    summary = {
        "asked": len(items),
        "in_materials": sum(1 for i in items if i["in_materials"] == "yes"),
        "plan_listed": sum(1 for i in items if i["plan_listed"] == "yes"),
        "taught_before_test": sum(1 for i in items if i["taught_before_test"] is True),
        "untaught": len(untaught),
        "untaught_on_map": sum(1 for i in untaught if i["map"] and i["map"]["on_map"]),
        "untaught_asked_by_new_plan": covered,
        "untaught_flagged_by_runtime": flagged_count,
        "untaught_silent": silent,
        "course_map": probe["state"],
        "map_built_from": built,
        "links": "none" if links is None else len(links),
    }
    result = {"check": "coverage", "fixture": fixture.name, "adapter": adapter.name, "as_of": fixture.as_of,
              "test": {"label": fixture.test_label, "at": fixture.test_at}, "target": fixture.target,
              "summary": summary, "map_probe": probe, "items": items, "links": links,
              "basis": fixture.asked().get("basis")}
    test = f"{fixture.test_label} on {fixture.test_at}" if fixture.test_at else "no test (a goal only)"
    lines = [
        f"Check 1, coverage: {fixture.name}. Plan as of {fixture.as_of or 'no date'}, folder {fixture.target or 'none'}; {test}.",
        f"Course map: {built}.",
        "",
        table(["Item", "What was asked", "In the materials", "On the map", "Asked by the plan", "Taught before", "Runtime flagged it"], rows,
              {1: 46}),
        "",
        f"Asked {summary['asked']}. In the materials {summary['in_materials']}. Listed by the plan {summary['plan_listed']}. "
        f"Taught before the test {summary['taught_before_test']}. Never taught {summary['untaught']}.",
        f"Of the {summary['untaught']} never taught: on the map {summary['untaught_on_map']}, asked by every planned task they need {covered}, "
        f"flagged by the runtime as asked by no planned task and never asked {flagged_count}, with no warning at all {silent}.",
        "'Runtime flagged it' means the map holds the item in scope, the record has no answer on it, and no task in the map's schedule asks or carries it, "
        "so `magnesium map` and course_map read list it. 'Asked by the plan' counts the matched items a planned task asks or carries.",
        "'The plan listed it' looks for the item's plan_refs in the plan's own files (plan.md, map.md, drills.md, gap-log.md), not in its papers.",
        f"Links: {summary['links']}.",
        "'Taught before' comes from the fixture, written by hand with its evidence; the other columns are worked out here.",
    ]
    extra = reach_lines(adapter, place, fixture, flags or {})
    if extra:
        lines += [""] + extra
    return result, "\n".join(lines)
