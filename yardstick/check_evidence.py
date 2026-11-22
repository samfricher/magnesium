from __future__ import annotations

from common import Fixture, answered, moment, save, table

FIELDS = ("A 'blank-page pass' here is today's nearest record: mark 'right' (the tutor's first_try), "
          "help 'none' and cold true, from `magnesium attempts --json`. The record does not say whether the task "
          "supplied a step or whether the item was explained earlier in the sitting.")


def matches(attempt: dict, tags: list[str]) -> bool:
    keys = {t.lower() for t in tags}
    return (attempt.get("label") or "").lower() in keys or (attempt.get("kind") or "").lower() in keys


def labels_from(progress: dict, tags: list[str]) -> list[str]:
    keys = {t.lower() for t in tags}
    skills = list((progress.get("other") or {}).get("skills", []))
    for plan in progress.get("plans", []):
        skills += plan.get("skills", []) or []
    out = []
    for skill in skills:
        if (skill.get("skill") or "").lower() not in keys:
            continue
        words = [f"{skill.get('passes', 0)} of 3 passes"]
        if skill.get("reading"):
            words.insert(0, skill["reading"])
        if skill.get("proven"):
            words.append("proven")
        if skill.get("due"):
            words.append("due")
        elif skill.get("next_on"):
            words.append(f"next {skill['next_on']}")
        if skill.get("needs_repair"):
            words.append("needs repair")
        out.append(f"{skill['skill']}: {', '.join(words)}")
    return out


def record_progress(args, adapter, fixture: Fixture) -> tuple[dict | None, str]:
    place = adapter.place_with_record(fixture.folder, f"evidence-{fixture.name}-record")
    if place:
        return adapter.progress(place), "progress --json on a copy of the fixture's own record"
    script = fixture.record_script()
    if script:
        place = adapter.place(fixture.folder, f"evidence-{fixture.name}-replay")
        adapter.seed(place, script["turns"])
        if getattr(args, "refresh_attempts", False):
            save(fixture.path / fixture.data.get("attempts", "attempts.json"), {"attempts": adapter.attempts(place)})
        return adapter.progress(place), "progress --json on a fresh replay of the fixture's scripted record, run today"
    return None, "no record in the fixture (no .magnesium folder and no scripted record), so no runtime label"


def run(args, adapter) -> tuple[dict, str]:
    fixture = Fixture(args.fixture)
    progress, label_source = record_progress(args, adapter, fixture)
    attempts = fixture.attempts()
    cutoff = fixture.test_at
    limit = moment(cutoff)
    rows, items = [], []
    for item in fixture.asked().get("items", []):
        tags = item.get("record_tags", [])
        before = [a for a in attempts if matches(a, tags) and (limit is None or (answered(a) and answered(a) < limit))]
        clean = [a for a in before if a.get("mark") == "right" and a.get("help") == "none" and a.get("cold") is True]
        helped = [a for a in before if a.get("help") not in (None, "none")]
        last = max((a.get("answered_at", "") for a in before), default=None)
        labels = labels_from(progress, tags) if progress is not None else []
        label = "; ".join(labels) if labels else ("no skill of this name" if progress is not None else "no record")
        items.append({"id": item["id"], "label": item.get("label"), "record_tags": tags, "marked": len(before),
                      "blank_page": len(clean), "helped": len(helped), "last": last, "runtime_label": label,
                      "answers": [{k: a.get(k) for k in ("answered_at", "label", "kind", "mark", "help", "cold", "question")} for a in before]})
        rows.append([item["id"], item.get("label"), len(before), len(clean), len(helped), last or "never", label])
    warn = [i for i in items if i["marked"] >= 2 and i["blank_page"] == 0]
    result = {"check": "evidence", "fixture": fixture.name, "adapter": adapter.name,
              "test": {"label": fixture.test_label, "at": cutoff}, "fields": FIELDS, "label_source": label_source,
              "attempts_in_fixture": len(attempts), "items": items,
              "marked_often_never_blank_page": [i["id"] for i in warn]}
    when = f"before {fixture.test_label} at {cutoff}" if cutoff else "with no test date, all answers"
    lines = [
        f"Check 2, evidence: {fixture.name}, {when}. {len(attempts)} answers in the fixture's record.",
        "",
        table(["Item", "What was asked", "Marked", "Right, no help, cold", "Helped", "Last", "The runtime's label"], rows, {1: 40, 6: 70}),
        "",
        FIELDS,
        f"Label: {label_source}.",
        ("Marked twice or more with no blank-page pass: " + ", ".join(i["id"] for i in warn) + ".") if warn else
        "No item was marked twice or more without a blank-page pass.",
    ]
    return result, "\n".join(lines)
