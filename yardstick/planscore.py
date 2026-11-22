from __future__ import annotations

import re
from pathlib import Path

from common import mentions, page_range

ROW = re.compile(r"^\s*(\|(?![\s|:-]*$)|[-*+]\s+|\d+[.)]\s+)")
PRACTICE = re.compile(r"\b(drill|drills|exercise|exercises|ex\.|problem|problems|question|questions|practice|quiz|small test|section test|redo|worked|task|tasks|mock|paper)\b", re.I)
LABEL = re.compile(r"#?|\d{1,3}[a-z]?|[A-Za-z]{1,2}-?\w{0,6}\d\w{0,3}|[A-Z]|row|item|step|block|week", re.I)
WRITTEN = re.compile(r"\b(drill|drills|mock|mocka|mockb|diagnostic|tutor[- ]written|generated|bucket 3|papers/)\b", re.I)
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


def score(folder: Path, lists: dict, judge=None) -> dict:
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

    per_section = []
    for section in sections:
        mine = [b for b in plan_blocks if section["id"] in b["names"]]
        body = "\n".join(b["body"] for b in mine)
        listed = posed_in(body, in_scope_ids)
        written = WRITTEN.search(body)
        ok = bool(mine) and bool(listed) and (written is None or listed[0][0] <= written.start())
        per_section.append({"id": section["id"], "blocks": [where(b["line"]) for b in mine], "pass": ok,
                            "first_listed": listed[0][1] if listed else None, "first_written": written.group(0) if written else None})
    counts.append({
        "key": "listed_before_written", "label": "Sections whose blocks pose a listed exercise before any tutor-written problem",
        "method": "mechanical: within each section's blocks, the first posed listed exercise against the first drill, mock or Diagnostic",
        "value": sum(1 for x in per_section if x["pass"]), "of": len(sections), "items": per_section})

    uncovered = [x for x in idea_hits if not x["row"]]
    cut_lines = [line for line in lines if CUT.search(line["text"])]
    judged = judge(folder, lines, sections, uncovered, cut_lines) if judge else None
    for key, label in (("sections_small_tests", "Sections that are each a small test on unseen problems"),
                       ("carried_ideas_named", "Ideas with no question of their own whose carrying problem is named"),
                       ("cuts_named", "Cuts named by their content")):
        entry = {"key": key, "label": label, "method": "judged"}
        if judged is None:
            entry.update({"value": None, "of": None, "note": "not run: pass --judge"})
        else:
            entry.update(judged.get(key, {"value": None, "of": None}))
        counts.append(entry)
    return {"plan": str(folder), "files": [str(p.relative_to(folder)) for p in plan_files(folder)], "counts": counts}


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

1. sections: one entry for every course section listed under SECTIONS, none left out. For each, say whether the plan gives that section its own small test on problems the learner has not seen (a short set of changed or new problems done without help), and quote the plan sentence or row it rests on. If the section has no such test, small_test is false and the quote is the closest row, or "" if none.
2. carried: one entry for every idea listed under IDEAS WITH NO PRACTICE ROW. For each, say whether the plan names a larger problem or exercise that carries that idea (for example "carried by exercise 2.1"), and quote it.
3. cuts: list every cut, drop or omission the plan states (including a "drop order"), and for each say whether it is named by its content (what is left out, in the course's terms) rather than only by a number, a block name or "the rest". Judge by the words of the cut's own sentence: a cut that names only numbers or block numbers is not named by content, even when those blocks are described elsewhere. Quote each.

Judge only from the text. Do not reward intentions that the plan does not write down."""

SCHEMA = {
    "type": "object",
    "properties": {
        "sections": {"type": "array", "items": {"type": "object", "properties": {
            "section": {"type": "string"}, "small_test": {"type": "boolean"}, "quote": {"type": "string"}},
            "required": ["section", "small_test", "quote"]}},
        "carried": {"type": "array", "items": {"type": "object", "properties": {
            "idea": {"type": "string"}, "named": {"type": "boolean"}, "quote": {"type": "string"}},
            "required": ["idea", "named", "quote"]}},
        "cuts": {"type": "array", "items": {"type": "object", "properties": {
            "cut": {"type": "string"}, "named_by_content": {"type": "boolean"}, "quote": {"type": "string"}},
            "required": ["cut", "named_by_content", "quote"]}},
    },
    "required": ["sections", "carried", "cuts"],
}


def judge_prompt(folder: Path, sections: list[dict], uncovered: list[dict], limit: int = 120000) -> str:
    body = []
    for path in plan_files(folder):
        try:
            body.append(f"=== {path.relative_to(folder)} ===\n{path.read_text(encoding='utf-8')}")
        except UnicodeDecodeError:
            continue
    text = "\n\n".join(body)[:limit]
    listed = "\n".join(f"- {s['id']}: {s.get('label')}" for s in sections) or "- none"
    ideas = "\n".join(f"- {u['id']}: {u['title']}" for u in uncovered) or "- none"
    return f"{RUBRIC}\n\nSECTIONS\n{listed}\n\nIDEAS WITH NO PRACTICE ROW\n{ideas}\n\nPLAN FILES\n{text}"


def read_judged(answer: dict) -> dict:
    sections = answer.get("sections") or []
    carried = answer.get("carried") or []
    cuts = answer.get("cuts") or []
    return {
        "sections_small_tests": {"value": sum(1 for s in sections if s.get("small_test")), "of": len(sections), "items": sections},
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
