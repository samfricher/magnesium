#!/usr/bin/env python3
"""Check the /magnesium:help topics and example screens against the plain-English rules.

Usage: check_help_text.py [SKILL.md]. Prints one line per problem and exits 1 when any exist.
"""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
SKILL = ROOT / "claude-plugin/skills/help/SKILL.md"
TOPICS = ("overview", "papers", "recording", "setup", "trouble")
WORD_LIMITS = {"Topic: overview": 180}
TOPIC_WORDS = 250
SCREEN_WORDS = 180
SENTENCE_MAX = 20
BANNED = {
    "long dash": re.compile(r"[‒–—―]|(?<=\S) --? (?=\S)"),
    "curly quote": re.compile(r"[‘’“”]"),
    "semicolon": re.compile(r";"),
    "markdown link": re.compile(r"!?\[[^\]]*\]\([^)]*\)|^\s*\[[^\]]+\]:\s"),
    "abbreviation": re.compile(r"\b(?:e\.g|i\.e|etc)\b"),
}
HEADING = re.compile(r"#{1,6} (.+)")
FENCE = re.compile(r"(`{3,})(\w*)")
MARKER = re.compile(r"^\s*(?:#{1,6}\s+|[-*+]\s+|\d+\.\s+|>\s*)")
SENTENCE_END = re.compile(r"(?<=[.!?])\s+|(?<=[.!?][\"')])\s+")
TABLE_RULE = re.compile(r"\|[\s|:-]+\|?")


def fenced_sections(text: str) -> dict[str, str]:
    """Map each heading to the body of the first markdown fence under it."""
    sections: dict[str, str] = {}
    heading, fence, language, body = "", "", "", []
    for line in text.splitlines():
        if fence:
            if line == fence:
                if language == "markdown":
                    sections.setdefault(heading, "\n".join(body) + "\n")
                fence, body = "", []
            else:
                body.append(line)
        elif match := FENCE.fullmatch(line):
            fence, language = match.groups()
        elif match := HEADING.fullmatch(line):
            heading = match.group(1)
    return sections


def words(text: str) -> list[str]:
    return [token for token in text.split() if re.search(r"[A-Za-z0-9]", token)]


def code_as_words(text: str) -> str:
    """Count `/magnesium:help papers` as two words, so a code span cannot hide sentence length."""
    return re.sub(r"`([^`]*)`", lambda match: " ".join("C" for _ in match.group(1).split()) or "C", text)


def sentences(lines: list[str]) -> list[str]:
    units, current, in_fence = [], [], False
    for line in lines + [""]:
        stripped = line.strip()
        if stripped.startswith("```"):
            in_fence = not in_fence
            stripped = ""
        if in_fence:
            continue
        if not stripped or stripped.startswith("|") or MARKER.match(line) or stripped.startswith("Next:"):
            units.append(" ".join(current))
            current = []
        if stripped.startswith("|"):
            if not TABLE_RULE.fullmatch(stripped):
                units.extend(cell for cell in stripped.strip("|").split("|"))
        elif stripped:
            current.append(MARKER.sub("", line).strip())
    return [
        sentence.strip()
        for unit in units
        for sentence in SENTENCE_END.split(code_as_words(unit.replace("**", "")))
        if words(sentence)
    ]


def check(name: str, text: str, limit: int) -> list[str]:
    lines = text.splitlines()
    found = []
    count = len(words(" ".join(re.sub(r"[|*#`]", " ", MARKER.sub("", line)) for line in lines)))
    if count > limit:
        found.append(f"{count} words, over the limit of {limit}")
    for sentence in sentences(lines):
        if len(words(sentence)) > SENTENCE_MAX:
            found.append(f"sentence of {len(words(sentence))} words: {sentence!r}")
    for line in lines:
        found.extend(f"{rule}: {line.strip()!r}" for rule, pattern in BANNED.items() if pattern.search(line))
    last = next((line for line in reversed(lines) if line.strip()), "")
    if sum(line.startswith("Next:") for line in lines) != 1 or not last.startswith("Next:"):
        found.append("needs exactly one Next: line, and it must close the text")
    return [f"{name}: {problem}" for problem in found]


def problems(text: str) -> list[str]:
    sections = fenced_sections(text)
    found = [f"Topic: {topic}: missing" for topic in TOPICS if f"Topic: {topic}" not in sections]
    for name, body in sections.items():
        if name.startswith("Topic: "):
            found += check(name, body, WORD_LIMITS.get(name, TOPIC_WORDS))
    shape = sections.get("Show the screen", "")
    examples = {name: body for name, body in sections.items() if name.startswith("Example: ")}
    if "## In this folder" not in shape or "## More help" not in shape or not examples:
        return found + ["Show the screen: the screen shape or its examples are missing"]
    head = shape.split("## In this folder")[0]
    more = "## More help" + shape.split("## More help")[1].split("Next:")[0]
    for name, body in examples.items():
        before, next_word, after = body.partition("Next:")
        found += check(name, f"{head}{before}{more}{next_word}{after}", SCREEN_WORDS)
    return found


def main(argv: list[str] | None = None) -> int:
    args = sys.argv[1:] if argv is None else argv
    path = Path(args[0]) if args else SKILL
    found = problems(path.read_text(encoding="utf-8"))
    for problem in found:
        print(problem, file=sys.stderr)
    return 1 if found else 0


if __name__ == "__main__":
    raise SystemExit(main())
