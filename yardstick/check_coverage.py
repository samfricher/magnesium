from __future__ import annotations

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


def run(args, adapter) -> tuple[dict, str]:
    fixture = Fixture(args.fixture)
    place = adapter.place(fixture.folder, f"coverage-{fixture.name}")
    probe = adapter.course_map(place, fixture.target)
    flagged = adapter.flagged_untaught(place, fixture.target)
    links = adapter.use_links(place, fixture.target)
    plan_folder = fixture.plan_folder()
    plan = own_plan_text(plan_folder)
    rows, items = [], []
    for item in fixture.asked().get("items", []):
        material, where = in_material(fixture, item.get("material", []), item.get("terms", []))
        wanted = item.get("plan_refs") or item.get("terms", [])
        refs = [r for r in wanted if plan and mentions(plan, r)]
        listed = "yes" if refs else ("no plan" if not plan else "no")
        taught = item.get("taught_before_test")
        was_flagged = None if flagged is None else item["id"] in flagged
        items.append({"id": item["id"], "label": item.get("label"), "in_materials": material, "where": where,
                      "plan_listed": listed, "plan_matches": refs, "taught_before_test": taught,
                      "runtime_flagged_untaught": was_flagged, "evidence": item.get("evidence")})
        rows.append([item["id"], item.get("label"), material, listed,
                     {True: "yes", False: "NO", None: "unknown"}[taught],
                     "no (no map)" if was_flagged is None else ("yes" if was_flagged else "no")])
    untaught = [i for i in items if i["taught_before_test"] is False]
    flagged_count = sum(1 for i in untaught if i["runtime_flagged_untaught"])
    summary = {
        "asked": len(items),
        "in_materials": sum(1 for i in items if i["in_materials"] == "yes"),
        "plan_listed": sum(1 for i in items if i["plan_listed"] == "yes"),
        "taught_before_test": sum(1 for i in items if i["taught_before_test"] is True),
        "untaught": len(untaught),
        "untaught_flagged_by_runtime": flagged_count,
        "course_map": probe["state"],
        "links": "none" if links is None else len(links),
        "reach": "none" if links is None else "see links",
    }
    result = {"check": "coverage", "fixture": fixture.name, "adapter": adapter.name, "as_of": fixture.as_of,
              "test": {"label": fixture.test_label, "at": fixture.test_at}, "target": fixture.target,
              "summary": summary, "map_probe": probe, "items": items,
              "basis": fixture.asked().get("basis")}
    test = f"{fixture.test_label} on {fixture.test_at}" if fixture.test_at else "no test (a goal only)"
    lines = [
        f"Check 1, coverage: {fixture.name}. Plan as of {fixture.as_of or 'no date'}, folder {fixture.target or 'none'}; {test}.",
        "",
        table(["Item", "What was asked", "In the materials", "The plan listed it", "Taught before", "Runtime flagged it untaught"], rows,
              {1: 46}),
        "",
        f"Asked {summary['asked']}. In the materials {summary['in_materials']}. Listed by the plan {summary['plan_listed']}. "
        f"Taught before the test {summary['taught_before_test']}. Never taught {summary['untaught']}.",
        f"Flagged as untaught by the runtime: {flagged_count} of {summary['untaught']}. "
        + ("The runtime has no course map, so it cannot flag anything." if probe["state"] == "none" else ""),
        f"Course map: {probe['state']} (looked for a map tool, a map command and {probe['map_page'] or 'a map page, with no plan folder to hold one'}).",
        "'The plan listed it' looks for the item's plan_refs in the plan's own files (plan.md, graph.yaml, drills.md, gap-log.md), not in its papers.",
        f"Links: {summary['links']}. Reach: {summary['reach']}.",
        "'Taught before' comes from the fixture, written by hand with its evidence; the other columns are worked out here.",
    ]
    return result, "\n".join(lines)
