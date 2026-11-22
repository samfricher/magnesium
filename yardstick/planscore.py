from __future__ import annotations

import json
import re
from pathlib import Path

from common import mentions, page_range

ROW = re.compile(r"^\s*(\|(?![\s|:-]*$)|[-*+]\s+|\d+[.)]\s+)")
PRACTICE = re.compile(r"\b(drill|drills|exercise|exercises|ex\.|problem|problems|question|questions|practice|quiz|small test|section test|redo|worked|task|tasks|mock|paper)\b", re.I)
LABEL = re.compile(r"#?|\d{1,3}[a-z]?|[A-Za-z]{1,2}-?\w{0,6}\d\w{0,3}|[A-Z]|row|item|step|block|week", re.I)
SLIDES = re.compile(r"\b(?:slides?|pages?|pp?\.)\s*(\d+(?:\s*(?:to|-|–)\s*\d+)?(?:\s*(?:,|and)\s*\d+(?:\s*(?:to|-|–)\s*\d+)?)*)", re.I)
SPAN = re.compile(r"(\d+)(?:\s*(?:to|-|–)\s*(\d+))?")
EXWORD = re.compile(r"(exercises?|ex\.?|#|problems?|questions?|q|lists?|items?|homework|hw|carries|carried by|carry)\s*[:#]?\s*(?:[\w.]+\s*(?:,|and|&)\s*)*$", re.I)
HEADING = re.compile(r"^\s*#{1,6}\s+\S")
WIDE = 4
CUT = re.compile(r"\b(cut|cuts|drop|dropped|drop order|left out|leave out|skip|skipped|out of scope|not covered)\b", re.I)


def plan_files(folder: Path) -> list[Path]:
    files = [p for p in sorted(Path(folder).rglob("*")) if p.is_file() and p.suffix in (".md", ".yaml", ".yml", ".txt")]
    files = [p for p in files if not any(part.startswith(".") for part in p.relative_to(folder).parts) and "." not in p.stem]
    main = [p for p in files if p.name == "plan.md" and p.parent == Path(folder)]
    return main + [p for p in files if p not in main]


def lines_of(folder: Path) -> list[dict]:
    out = []
    for path in plan_files(folder):
        name = str(path.relative_to(folder))
        drills = path.name.startswith("drills") or "papers" in path.parts
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        for n, line in enumerate(text.splitlines()):
            out.append({"file": name, "line": n + 1, "text": line, "row": bool(ROW.match(line)), "drills": drills})
    return out


def pages_in(text: str) -> set[int]:
    found = set()
    for match in SLIDES.finditer(text):
        for span in SPAN.finditer(match.group(1)):
            start = int(span.group(1))
            end = int(span.group(2) or start)
            if 0 < end - start < 200:
                found.update(range(start, end + 1))
            else:
                found.add(start)
    return found


def names(exercise: dict) -> list[str]:
    return [exercise["id"], *exercise.get("aliases", [])]


def id_pattern(name: str) -> re.Pattern:
    return re.compile(r"(?<![\w.])" + re.escape(name) + r"(?![\w]|\.\d)", re.I)


RANGE = re.compile(r"(?<![\w.])(\d+)\.(\d+)\s*(?:-|–|to)\s*(?:(\d+)\.)?(\d+)(?![\w]|\.\d)")


def in_range(text: str, name: str) -> bool:
    parts = name.split(".")
    if len(parts) != 2 or not all(p.isdigit() for p in parts):
        return False
    for match in RANGE.finditer(text):
        head, low, other, high = match.group(1), int(match.group(2)), match.group(3), int(match.group(4))
        if (other is None or other == head) and parts[0] == head and low < int(parts[1]) < high and high - low <= 30:
            return True
    return False


def names_exercise(text: str, name: str, others: list[str]) -> bool:
    if in_range(text, name):
        return True
    for match in id_pattern(name).finditer(text):
        before = text[max(0, match.start() - 40): match.start()]
        if EXWORD.search(before):
            return True
        cell = [c.strip() for c in text.split("|")]
        if name in cell:
            return True
        if any(o != name and id_pattern(o).search(text) for o in others):
            return True
        if re.search(r"\b(exercise|ex\.?)\b", text, re.I) and not re.match(r"\s*(h|min|hours?|%|marks?)\b", text[match.end():]):
            return True
    return False


def subject(text: str) -> str:
    stripped = text.strip()
    if stripped.startswith("|"):
        for cell in (c.strip() for c in stripped.strip("|").split("|")):
            if not LABEL.fullmatch(cell) and not re.fullmatch(r"[-:\s]*", cell):
                return cell
        return ""
    return ROW.sub("", text, count=1)[:80]


def listed_on(text: str, ids: list[str]) -> set[str]:
    return {n for n in ids if in_range(text, n) or id_pattern(n).search(text)}


def own_row(text: str, name: str, ids: list[str]) -> bool:
    head = subject(text)
    if text.strip().startswith("|"):
        if head and (in_range(head, name) or id_pattern(name).search(head)):
            return True
        return len(listed_on(text, ids)) <= 3 and names_exercise(text, name, ids)
    first = head.split()[0].rstrip(",:;") if head.split() else ""
    return first == name or in_range(first, name) or (len(listed_on(text, ids)) <= 3 and names_exercise(text, name, ids))


def practice_rows(lines: list[dict], exercise_ids: list[str]) -> list[dict]:
    rows = []
    for line in lines:
        if not line["row"]:
            continue
        if line["drills"] or PRACTICE.search(line["text"]) or any(names_exercise(line["text"], e, exercise_ids) for e in exercise_ids):
            rows.append(line)
    return rows


def covers(line: dict, terms: list[str], pages: list[int]) -> bool:
    seen = pages_in(line["text"])
    if pages and seen and len(seen) <= WIDE and seen & set(pages):
        return True
    return any(mentions(line["text"], term) for term in terms if term)


def sections_named(text: str, sections: list[dict], by_section: dict, all_ids: list[str]) -> set[str]:
    seen = pages_in(text)
    found = set()
    for section in sections:
        if re.search(rf"\bsection {re.escape(section['id'])}(?![\w.])", text, re.I):
            found.add(section["id"])
        elif section.get("label") and mentions(text, section["label"]):
            found.add(section["id"])
        elif seen and seen & page_range(section.get("slide_pages")):
            found.add(section["id"])
        elif any(names_exercise(text, n, all_ids) for n in by_section.get(section["id"], [])):
            found.add(section["id"])
    return found if len(found) * 2 <= max(len(sections), 2) else set()


SCHEDULE_HEAD = re.compile(r"\b(block|blocks|session|sessions|step|steps|day|days|date|when|duration|stop line|sitting|running total|target day)\b", re.I)
SEPARATOR = re.compile(r"^\s*\|[\s|:-]+\|?\s*$")
POSES = re.compile(r"\b(exercises?|ex\.?|wording|changed versions?|variations?|as written|as the book)\b", re.I)
CLAUSE = re.compile(r";|\(|\)|\||\n|\.\s|:\s(?=[A-Z])")


def heading_level(text: str) -> int:
    return len(text) - len(text.lstrip("#")) if HEADING.match(text) else 0


def blocks(lines: list[dict], sections: list[dict], by_section: dict, all_ids: list[str]) -> list[dict]:
    plan = [line for line in lines if line["file"] == "plan.md"]
    out = []
    schedule_table = False
    schedule_part = False
    current = None
    for n, line in enumerate(plan):
        text = line["text"]
        level = heading_level(text)
        if level:
            named = sections_named(text, sections, by_section, all_ids)
            current = {"kind": "heading", "line": line, "level": level, "names": named, "text": [text]} if len(named) == 1 else None
            if current:
                out.append(current)
            schedule_part = False
            schedule_table = False
            continue
        if current is not None:
            current["text"].append(text)
        if text.strip().startswith("|"):
            following = plan[n + 1]["text"] if n + 1 < len(plan) else ""
            if SEPARATOR.match(following):
                schedule_table = bool(SCHEDULE_HEAD.search(text))
                schedule_part = schedule_part or schedule_table
                continue
            if SEPARATOR.match(text):
                continue
            if schedule_table:
                out.append({"kind": "row", "line": line, "names": sections_named(text, sections, by_section, all_ids), "text": [text]})
            continue
        schedule_table = False
        if schedule_part and line["row"]:
            out.append({"kind": "list", "line": line, "names": sections_named(text, sections, by_section, all_ids), "text": [text]})
    for block in out:
        block["body"] = "\n".join(block.pop("text"))
    return out


def posed_in(text: str, ids: list[str]) -> list[tuple[int, str]]:
    found = []
    start = 0
    for piece in CLAUSE.split(text):
        at = text.find(piece, start)
        start = at + len(piece) if at >= 0 else start
        if not POSES.search(piece) or re.search(r"\bcarr", piece, re.I):
            continue
        for name in ids:
            match = id_pattern(name).search(piece)
            if match or in_range(piece, name):
                found.append(((at if at >= 0 else 0) + (match.start() if match else 0), name))
    return sorted(found)


def where(line: dict | None) -> str | None:
    return f"{line['file']}:{line['line']}" if line else None


def score(folder: Path, lists: dict, judge=None, marker=None) -> dict:
    lines = lines_of(folder)
    exercises = [e for e in (lists.get("exercises") or {}).get("exercises", []) if e.get("in_scope", True)]
    all_ids = [n for e in (lists.get("exercises") or {}).get("exercises", []) for n in names(e)]
    rows = practice_rows(lines, all_ids)
    in_scope_ids = [n for e in exercises for n in names(e)]
    counts = []

    sections = (lists.get("order") or {}).get("sections", [])
    by_section = {}
    for e in exercises:
        by_section.setdefault(e.get("section"), []).extend(names(e))
    plan_blocks = blocks(lines, sections, by_section, all_ids)
    posed = {}
    for block in plan_blocks:
        for _, name in posed_in(block["body"], in_scope_ids):
            posed.setdefault(name, where(block["line"]))
    items = [{"id": e["id"], "posed": next((posed[n] for n in names(e) if n in posed), None)} for e in exercises]
    counts.append({
        "key": "listed_exercises_posed", "label": "In-scope listed exercises a schedule block poses itself",
        "method": "mechanical: named in a timed or section block, in a clause that sets it (exercise, wording, changed version), not one that says another problem carries it",
        "value": sum(1 for x in items if x["posed"]), "of": len(exercises), "items": items})
    with_row = []
    for e in exercises:
        hit = next((line for line in lines if line["row"] and any(own_row(line["text"], n, in_scope_ids) for n in names(e))), None)
        mention = next((line for line in lines if any(names_exercise(line["text"], n, all_ids) for n in names(e))), None)
        with_row.append({"id": e["id"], "row": where(hit), "mentioned": where(mention)})
    counts.append({
        "key": "listed_exercises_with_row", "label": "In-scope listed exercises with a row of their own",
        "method": "mechanical: the number in the first cell of a table row or at the start of a list line, or on a row naming at most three listed exercises",
        "value": sum(1 for x in with_row if x["row"]), "of": len(exercises), "items": with_row})
    counts.append({
        "key": "listed_exercises_mentioned", "label": "In-scope listed exercises mentioned anywhere",
        "method": "mechanical: the number appears as an exercise in any plan file",
        "value": sum(1 for x in with_row if x["mentioned"]), "of": len(exercises)})

    ideas = (lists.get("slide-ideas") or {}).get("ideas", [])
    idea_hits = []
    for idea in ideas:
        pages = sorted(page_range(idea.get("pages")) or page_range(idea.get("page")))
        hit = next((line for line in rows if covers(line, idea.get("terms", []) + [idea.get("title", "")], pages)), None)
        idea_hits.append({"id": idea["id"], "title": idea.get("title"), "row": where(hit)})
    counts.append({
        "key": "slide_ideas_with_practice", "label": "Slide ideas with a practice item",
        "method": "mechanical: a practice row names the idea's slide page or one of its terms",
        "value": sum(1 for x in idea_hits if x["row"]), "of": len(ideas), "items": idea_hits})

    added = (lists.get("added-items") or {}).get("items", [])
    added_hits = []
    for item in added:
        hit = next((line for line in rows if covers(line, item.get("terms", []), item.get("slide_pages", []))), None)
        added_hits.append({"id": item["id"], "idea": item.get("idea"), "row": where(hit)})
    counts.append({
        "key": "added_items_with_practice", "label": "Added items (ideas with no listed exercise) with a practice item",
        "method": "mechanical: a practice row names the item's slide page or one of its terms",
        "value": sum(1 for x in added_hits if x["row"]), "of": len(added), "items": added_hits})

    first = []
    for n, section in enumerate(sections):
        hit = next((i for i, block in enumerate(plan_blocks) if section["id"] in block["names"]), None)
        first.append({"id": section["id"], "label": section.get("label"), "lecture": n, "at": hit,
                      "first": where(plan_blocks[hit]["line"]) if hit is not None else None})
    found = [f for f in first if f["at"] is not None]
    order = [f["lecture"] for f in sorted(found, key=lambda f: f["at"])]
    counts.append({
        "key": "sections_in_lecture_order", "label": "Sections in lecture order in the schedule",
        "method": "mechanical: the order of the first timed block or section heading that names each section",
        "value": longest_rising(order), "of": len(sections), "found": len(found), "order_seen": [sections[i]["id"] for i in order], "items": first})

    uncovered = [x for x in idea_hits if not x["row"]]
    cut_lines = [line for line in lines if CUT.search(line["text"])]
    judged = judge(folder, lines, sections, uncovered, cut_lines) if judge else None
    for key, label in (("carried_ideas_named", "Ideas with no question of their own whose carrying problem is named"),
                       ("cuts_named", "Cuts named by their content")):
        entry = {"key": key, "label": label, "method": "judged"}
        if judged is None:
            entry.update({"value": None, "of": None, "note": "not run: pass --judge"})
        else:
            entry.update(judged.get(key, {"value": None, "of": None}))
        counts.append(entry)
    result = {"plan": str(folder), "files": [str(p.relative_to(folder)) for p in plan_files(folder)], "counts": counts,
              "units": units(folder, lists)}
    if marker:
        runs = marker(folder, result["units"])
        marks = majority(runs, result["units"])
        result["block_runs"] = runs
        result["blocks"] = marks
        result["block_spread"] = spread_of(runs, result["units"])
        result["shape"] = shape(marks, sections, in_scope_ids)
        result["shape_runs"] = [shape(run, sections, in_scope_ids) for run in runs]
    return result


KINDS = ["tutor_paper", "instructor_paper", "teach_first", "test_first", "marking", "chosen_by_marks", "redo", "other"]
WORK = ("teach_first", "test_first")
SITS = re.compile(r"\b(test|tests|paper|papers|mock|exam|diagnostic)\b", re.I)
CODEBOOK = Path(__file__).with_name("plan-blocks.md")


def units(folder: Path, lists: dict) -> list[dict]:
    sections = (lists.get("order") or {}).get("sections", [])
    every = (lists.get("exercises") or {}).get("exercises", [])
    all_ids = [n for e in every for n in names(e)]
    by_section: dict = {}
    for e in every:
        if e.get("in_scope", True):
            by_section.setdefault(e.get("section"), []).extend(names(e))
    lines = lines_of(folder)
    found = blocks(lines, sections, by_section, all_ids)
    rows = [b for b in found if b["kind"] == "row"]
    if rows:
        out = []
        for b in rows:
            cells = [c.strip() for c in b["line"]["text"].strip().strip("|").split("|")]
            out.append({"block": cells[0], "title": cells[1] if len(cells) > 1 else "", "line": where(b["line"])})
        return out
    headings = [b for b in found if b["kind"] == "heading"]
    out = [{"block": b["line"]["text"].lstrip("#").strip(), "title": b["line"]["text"].lstrip("#").strip(), "line": where(b["line"])}
           for b in headings]
    if not headings:
        return out
    plan = [line for line in lines if line["file"] == "plan.md"]
    start = plan.index(headings[0]["line"])
    named = {id(b["line"]) for b in headings}
    heading, n = None, 0
    for line in plan[start:]:
        if heading_level(line["text"]):
            heading = None if id(line) in named else line["text"].lstrip("#").strip()
            n = 0
            continue
        if heading and ROW.match(line["text"]) and SITS.search(line["text"]) and not CUT.search(line["text"]):
            n += 1
            out.append({"block": f"{heading} {n}", "title": ROW.sub("", line["text"], count=1).strip()[:80], "line": where(line)})
    return out


def same_block(a: str, b: str) -> bool:
    return norm_block(a) == norm_block(b)


def norm_block(text: str) -> str:
    return re.sub(r"\s+", " ", str(text)).strip().strip("#").strip().lower()


def section_list(value, section_ids: list[str]) -> list[str]:
    if value == "all" or (isinstance(value, list) and "all" in value):
        return ["all"]
    return [s for s in section_ids if s in (value or [])]


def clean(mark: dict, section_ids: list[str], listed_ids: list[str]) -> dict:
    kind = mark.get("kind") if mark.get("kind") in KINDS else "other"
    unseen = mark.get("unseen")
    return {"block": str(mark.get("block", "")), "done": bool(mark.get("done")),
            "sections": section_list(mark.get("sections"), section_ids), "kind": kind,
            "listed": sorted({x for x in (mark.get("listed") or []) if x in listed_ids}, key=listed_ids.index),
            "unseen": unseen if kind in WORK and isinstance(unseen, bool) else None, "quote": mark.get("quote", "")}


def read_marks(answer: dict, plan_units: list[dict], section_ids: list[str], listed_ids: list[str]) -> list[dict]:
    given = [clean(m, section_ids, listed_ids) for m in (answer.get("blocks") or [])]
    out = []
    for n, unit in enumerate(plan_units):
        hit = next((m for m in given if same_block(m["block"], unit["block"])), None)
        if hit is None and len(given) == len(plan_units):
            hit = given[n]
        out.append(dict(hit or clean({"kind": "other"}, section_ids, listed_ids), block=unit["block"], missing=hit is None))
    return out


def vote(values: list):
    counted: dict = {}
    for v in values:
        key = json.dumps(v, sort_keys=True)
        counted[key] = counted.get(key, 0) + 1
    best = max(counted.values())
    return json.loads(next(k for k in (json.dumps(v, sort_keys=True) for v in values) if counted[k] == best))


def majority(runs: list[list[dict]], plan_units: list[dict]) -> list[dict]:
    out = []
    for n, unit in enumerate(plan_units):
        marks = [run[n] for run in runs]
        need = len(marks) // 2 + 1
        sections = sorted({s for m in marks for s in m["sections"]}, key=lambda s: (s != "all", [x for m in marks for x in m["sections"]].index(s)))
        listed = sorted({x for m in marks for x in m["listed"]}, key=lambda x: [y for m in marks for y in m["listed"]].index(x))
        kind = vote([m["kind"] for m in marks])
        out.append({"block": unit["block"], "done": vote([m["done"] for m in marks]), "kind": kind,
                    "sections": sorted((s for s in sections if sum(s in m["sections"] for m in marks) >= need), key=lambda s: ids_order(s, marks)),
                    "listed": sorted((x for x in listed if sum(x in m["listed"] for m in marks) >= need), key=lambda x: ids_order(x, marks)),
                    "unseen": vote([m["unseen"] for m in marks]) if kind in WORK else None,
                    "quote": next((m["quote"] for m in marks if m["kind"] == kind), "")})
    return out


def ids_order(value: str, marks: list[dict]) -> tuple:
    return (0, float(value)) if re.fullmatch(r"\d+(\.\d+)?", value) else (1, value)


def same_set(a, b) -> bool:
    return (set(a) == set(b)) if isinstance(a, list) and isinstance(b, list) else a == b


def spread_of(runs: list[list[dict]], plan_units: list[dict]) -> dict:
    fields = ("done", "kind", "sections", "listed", "unseen")
    same = {f: 0 for f in fields}
    for n in range(len(plan_units)):
        for f in fields:
            values = [json.dumps(sorted(run[n][f]) if isinstance(run[n][f], list) else run[n][f], sort_keys=True) for run in runs]
            same[f] += len(set(values)) == 1
    return {"runs": len(runs), "blocks": len(plan_units), "identical_across_runs": same}


def shape(marks: list[dict], sections: list[dict], listed_ids: list[str]) -> list[dict]:
    ids = [s["id"] for s in sections]
    planned = [m for m in marks if not m["done"]]
    work = [m for m in planned if m["kind"] in WORK]
    first: dict = {}
    for n, m in enumerate(work):
        for s in m["sections"]:
            if s in ids:
                first.setdefault(s, (n, m))
    seen = sorted(first, key=lambda s: (first[s][0], ids.index(s)))
    start = next((n for n, m in enumerate(planned) if m["kind"] in WORK and any(s in ids for s in m["sections"])), len(planned))
    unseen = {s for m in work if m["unseen"] for s in m["sections"] if s in ids}
    listed = {x for m in planned for x in m["listed"] if x in listed_ids}
    return [
        {"key": "sections_worked", "label": "Sections with a block of their own work", "value": len(first), "of": len(ids)},
        {"key": "sections_in_order", "label": "Sections in lecture order (first block of each)",
         "value": longest_rising([ids.index(s) for s in seen]), "of": len(ids), "order_seen": seen},
        {"key": "sections_test_first", "label": "Sections whose first block opens with a test, not teaching",
         "value": sum(1 for s in first if first[s][1]["kind"] == "test_first"), "of": len(ids)},
        {"key": "sections_unseen", "label": "Sections with a problem he has not seen, done without help",
         "value": len(unseen), "of": len(ids)},
        {"key": "listed_set_or_carried", "label": "Listed exercises set or carried by a block",
         "value": len(listed), "of": len(listed_ids), "items": sorted(listed, key=listed_ids.index)},
        {"key": "tutor_paper_first", "label": "A paper the tutor wrote comes before the first section block",
         "value": any(m["kind"] == "tutor_paper" for m in planned[:start]), "of": None},
        {"key": "instructor_paper", "label": "The instructor's own past paper is sat",
         "value": any(m["kind"] == "instructor_paper" for m in planned), "of": None},
        {"key": "chosen_by_marks", "label": "Blocks whose work is picked by lost marks",
         "value": sum(1 for m in planned if m["kind"] == "chosen_by_marks"), "of": None},
    ]


def agree(key: list[dict], judged: list[dict]) -> dict:
    pairs = []
    for k in key:
        j = next((m for m in judged if same_block(m["block"], k["block"])), None)
        if j is not None:
            pairs.append((k, j))
    out = {"blocks_in_key": len(key), "blocks_matched": len(pairs)}
    planned = [(k, j) for k, j in pairs if not k["done"]]
    out["done"] = [sum(1 for k, j in pairs if k["done"] == j["done"]), len(pairs)]
    for field in ("kind", "listed"):
        out[field] = [sum(1 for k, j in planned if same_set(k[field], j[field])), len(planned)]
    work = [(k, j) for k, j in planned if k["kind"] in WORK]
    for field in ("sections", "unseen"):
        out[field] = [sum(1 for k, j in work if same_set(k[field], j[field])), len(work)]
    out["differ"] = []
    for k, j in pairs:
        fields = ("done",) + (() if k["done"] else ("kind", "listed")) + (("sections", "unseen") if k["kind"] in WORK and not k["done"] else ())
        diff = {f: [k[f], j[f]] for f in fields if not same_set(k[f], j[f])}
        if diff:
            out["differ"].append({"block": k["block"], **diff})
    return out


BLOCK_SCHEMA = {
    "type": "object",
    "properties": {"blocks": {"type": "array", "items": {"type": "object", "properties": {
        "block": {"type": "string"}, "done": {"type": "boolean"},
        "sections": {"type": "array", "items": {"type": "string"}},
        "kind": {"type": "string", "enum": KINDS},
        "listed": {"type": "array", "items": {"type": "string"}},
        "unseen": {"type": ["boolean", "null"]}, "quote": {"type": "string"}},
        "required": ["block", "done", "sections", "kind", "listed", "unseen", "quote"]}}},
    "required": ["blocks"],
}


def block_prompt(folder: Path, lists: dict, plan_units: list[dict], limit: int = 150000) -> str:
    sections = (lists.get("order") or {}).get("sections", [])
    listed = [e for e in (lists.get("exercises") or {}).get("exercises", []) if e.get("in_scope", True)]
    body = []
    for path in plan_files(folder):
        try:
            body.append(f"=== {path.relative_to(folder)} ===\n{path.read_text(encoding='utf-8')}")
        except UnicodeDecodeError:
            continue
    text = "\n\n".join(body)[:limit]
    section_text = "\n".join(f"- {s['id']}: {s.get('label')} (slides {s.get('slide_pages')})" for s in sections) or "- none"
    listed_text = ", ".join(f"{e['id']} (section {e.get('section')})" for e in listed) or "none"
    unit_text = "\n".join(f"- {u['block']}: {u['title']}" for u in plan_units)
    return (f"{CODEBOOK.read_text(encoding='utf-8')}\n\nMark every block below, in this order, one entry each, with `block` written "
            f"exactly as given. Write `sections` as a list of ids, or [\"all\"].\n\nSECTIONS\n{section_text}\n\n"
            f"LISTED EXERCISES\n{listed_text}\n\nBLOCKS\n{unit_text}\n\nPLAN FILES\n{text}")


def shape_text(result: dict) -> list[list]:
    rows = []
    for n, item in enumerate(result.get("shape") or []):
        runs = [r[n]["value"] for r in result.get("shape_runs") or []]
        shown = f"{item['value']} of {item['of']}" if item["of"] is not None else ("yes" if item["value"] is True else "no" if item["value"] is False else str(item["value"]))
        spread = sorted({str(v) for v in runs})
        rows.append([item["label"], shown, ", ".join(spread) if len(spread) > 1 else "same"])
    return rows


def longest_rising(values: list[int]) -> int:
    best: list[int] = []
    for value in values:
        lo, hi = 0, len(best)
        while lo < hi:
            mid = (lo + hi) // 2
            if best[mid] < value:
                lo = mid + 1
            else:
                hi = mid
        if lo == len(best):
            best.append(value)
        else:
            best[lo] = value
    return len(best)


RUBRIC = """You are scoring a study plan against a fixed rubric. Read the plan files below and answer in the JSON schema.

1. carried: one entry for every idea listed under IDEAS WITH NO PRACTICE ROW. For each, say whether the plan names a larger problem or exercise that carries that idea (for example "carried by exercise 2.1"), and quote it.
2. cuts: list every cut, drop or omission the plan states (including a "drop order"), and for each say whether it is named by its content (what is left out, in the course's terms) rather than only by a number, a block name or "the rest". Judge by the words of the cut's own sentence: a cut that names only numbers or block numbers is not named by content, even when those blocks are described elsewhere. Quote each.

Judge only from the text. Do not reward intentions that the plan does not write down."""

SCHEMA = {
    "type": "object",
    "properties": {
        "carried": {"type": "array", "items": {"type": "object", "properties": {
            "idea": {"type": "string"}, "named": {"type": "boolean"}, "quote": {"type": "string"}},
            "required": ["idea", "named", "quote"]}},
        "cuts": {"type": "array", "items": {"type": "object", "properties": {
            "cut": {"type": "string"}, "named_by_content": {"type": "boolean"}, "quote": {"type": "string"}},
            "required": ["cut", "named_by_content", "quote"]}},
    },
    "required": ["carried", "cuts"],
}


def judge_prompt(folder: Path, uncovered: list[dict], limit: int = 120000) -> str:
    body = []
    for path in plan_files(folder):
        try:
            body.append(f"=== {path.relative_to(folder)} ===\n{path.read_text(encoding='utf-8')}")
        except UnicodeDecodeError:
            continue
    text = "\n\n".join(body)[:limit]
    ideas = "\n".join(f"- {u['id']}: {u['title']}" for u in uncovered) or "- none"
    return f"{RUBRIC}\n\nIDEAS WITH NO PRACTICE ROW\n{ideas}\n\nPLAN FILES\n{text}"


def read_judged(answer: dict) -> dict:
    carried = answer.get("carried") or []
    cuts = answer.get("cuts") or []
    return {
        "carried_ideas_named": {"value": sum(1 for c in carried if c.get("named")), "of": len(carried), "items": carried},
        "cuts_named": {"value": sum(1 for c in cuts if c.get("named_by_content")), "of": len(cuts), "items": cuts},
    }


def counts_text(result: dict) -> list[list]:
    rows = []
    for c in result["counts"]:
        if c.get("value") is None:
            shown = c.get("note", "not run")
        else:
            shown = f"{c['value']} of {c['of']}"
            if c["key"] == "sections_in_lecture_order":
                shown += f" (found {c['found']}, seen as {' '.join(c['order_seen']) or 'none'})"
        rows.append([c["label"], shown, "judged" if c["method"] == "judged" else "mechanical"])
    return rows
