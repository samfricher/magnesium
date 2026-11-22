from __future__ import annotations

import json
import re
import statistics
import tempfile
from datetime import datetime
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent


def load(path: Path):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def save(path: Path, value) -> Path:
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return path


def default_out() -> Path:
    return Path(tempfile.gettempdir()) / "magnesium-yardstick" / "results"


def now_local() -> datetime:
    return datetime.now().astimezone()



def local_day(stamp: str) -> str:
    return datetime.fromisoformat(stamp.replace("Z", "+00:00")).astimezone().date().isoformat()


def moment(value: str | None) -> datetime | None:
    if not value:
        return None
    text = value.strip().replace("Z", "+00:00")
    try:
        stamp = datetime.fromisoformat(text if "T" in text else text.replace(" ", "T"))
    except ValueError:
        return None
    return stamp if stamp.tzinfo else stamp.astimezone()


def answered(attempt: dict) -> datetime | None:
    return moment(attempt.get("answered_at_iso") or attempt.get("answered_at"))


def page_range(value) -> set[int]:
    if not value:
        return set()
    if isinstance(value, int):
        return {value}
    if len(value) == 2 and value[1] >= value[0]:
        return set(range(value[0], value[1] + 1))
    return set(value)



class Fixture:
    def __init__(self, path: Path):
        self.path = Path(path).resolve()
        self.data = load(self.path / "fixture.json")
        self.name = self.data.get("name") or self.path.name
        self.as_of = self.data.get("as_of")
        test = self.data.get("test") or {}
        self.test_label = test.get("label")
        self.test_at = test.get("at")
        self.folder = self.path / self.data.get("folder", "folder")
        self.target = self.data.get("target")
        self.transcripts = self.data.get("transcripts", [])
        self.goal = self.data.get("goal")

    def file(self, key: str, fallback: str | None = None) -> Path | None:
        name = self.data.get(key, fallback)
        if not name:
            return None
        path = self.path / name
        return path if path.exists() else None

    def attempts(self) -> list[dict]:
        path = self.file("attempts", "attempts.json")
        return load(path).get("attempts", []) if path else []

    def asked(self) -> dict:
        path = self.file("asked", "asked.json")
        return load(path) if path else {"basis": None, "items": []}

    def record_script(self) -> dict | None:
        path = self.file("record")
        return load(path) if path else None

    def lists(self) -> dict | None:
        base = self.path / "lists"
        if not base.is_dir():
            return None
        found = {}
        for name in ("exercises", "slide-ideas", "added-items", "order"):
            path = base / f"{name}.json"
            if path.is_file():
                found[name] = load(path)
        return found

    def plans(self) -> dict[str, Path]:
        base = self.path / "plans"
        if not base.is_dir():
            return {}
        found = {}
        for folder in sorted(p for p in base.iterdir() if p.is_dir()):
            if (folder / "plan.md").is_file():
                found[folder.name] = folder
                continue
            for inner in sorted(p for p in folder.iterdir() if p.is_dir() and (p / "plan.md").is_file()):
                found[f"{folder.name}/{inner.name}"] = inner
        return found

    def plan_folder(self) -> Path | None:
        if not self.target:
            return None
        path = self.folder / self.target
        return path if path.is_dir() else None


def table(headers: list[str], rows: list[list], widths: dict[int, int] | None = None) -> str:
    widths = widths or {}
    cells = [[str(h) for h in headers]] + [["" if c is None else str(c) for c in row] for row in rows]
    for row in cells:
        for i, cell in enumerate(row):
            limit = widths.get(i)
            if limit and len(cell) > limit:
                row[i] = cell[: limit - 1] + "…"
    size = [max(len(row[i]) for row in cells) for i in range(len(headers))]
    lines = []
    for n, row in enumerate(cells):
        lines.append("  ".join(cell.ljust(size[i]) for i, cell in enumerate(row)).rstrip())
        if n == 0:
            lines.append("  ".join("-" * size[i] for i in range(len(headers))))
    return "\n".join(lines)


def spread(values: list[int]) -> dict:
    if not values:
        return {"count": 0}
    ordered = sorted(values)

    def at(q: float) -> int:
        return ordered[min(len(ordered) - 1, int(q * (len(ordered) - 1) + 0.5))]

    return {
        "count": len(ordered),
        "min": ordered[0],
        "p10": at(0.1),
        "median": int(statistics.median(ordered)),
        "p90": at(0.9),
        "max": ordered[-1],
        "mean": int(sum(ordered) / len(ordered)),
    }


def spread_text(s: dict) -> str:
    if not s.get("count"):
        return "none"
    return f"{s['p10']:,} to {s['p90']:,} (median {s['median']:,}, max {s['max']:,})"


WORD = re.compile(r"[a-z0-9]+(?:[.'][a-z0-9]+)*")


def norm(text: str) -> str:
    return " ".join(WORD.findall((text or "").lower()))


def mentions(text: str, phrase: str) -> bool:
    phrase = norm(phrase)
    if not phrase:
        return False
    return re.search(r"(?<![a-z0-9.])" + re.escape(phrase) + r"(?![a-z0-9]|\.[0-9])", norm(text)) is not None




def emit(out: Path, check: str, fixture_name: str, result: dict, text: str) -> Path:
    print(text)
    path = save(Path(out) / f"{check}-{fixture_name}.json", result)
    print(f"\nResult written to {path}")
    return path
