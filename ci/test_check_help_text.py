"""The /magnesium:help text checker passes the real skill and names each planted defect."""
import contextlib
import io
from pathlib import Path
import tempfile
import unittest

from ci import check_help_text

LONG_SENTENCE = "Your study files stay where they are, and Magnesium reads them in place each time you ask the tutor for help."
SEMICOLON_LINE = "The tutor asks first; i.e. it installs tectonic only after you agree."
LINK_LINE = "Put Magnesium's `target/release` folder on your PATH. See [setup](setup.md) for how."
DASH_LINE = "2. **Learn one week.** Say “Teach me week 6 from the slides” — now."
OVER_LIMIT = "Magnesium helps you study. " * 50

SHAPE = """# Magnesium help

## In this folder

- <1 to 3 short facts you saw>

## More help

- `/magnesium:help papers` shows how practice papers work.

Next: <action 1, with its words or command>.
"""
EXAMPLES = ("course with a test ahead", "plan in progress")


def topic(title: str, text: str = "A short plain sentence.", next_line: str = "Next: type `/magnesium:help`.") -> str:
    return f"# {title}\n\n{text}\n" + (f"\n{next_line}\n" if next_line else "")


def example(action: str = '1. **Learn one week.** Say "Teach me week 6 from the slides".') -> str:
    return f'## In this folder\n\n- A short plain fact.\n\n## What you can do now\n\n{action}\n\nNext: say "Teach me".\n'


def skill_text(changes: dict[str, str] | None = None) -> str:
    """A small skill laid out like claude-plugin/skills/help/SKILL.md, with some section bodies replaced."""
    bodies = {"Show the screen": SHAPE}
    bodies.update({f"Topic: {name}": topic(name) for name in check_help_text.TOPICS})
    bodies.update({f"Example: {name}": example() for name in EXAMPLES})
    assert (changes or {}).keys() <= bodies.keys()
    bodies.update(changes or {})
    sections = []
    for name, body in bodies.items():
        fence = "````" if name.startswith("Topic: ") else "```"
        sections.append(f"## {name}\n\n{fence}markdown\n{body}{fence}\n")
    return "\n".join(sections)


class HelpTextCheckTests(unittest.TestCase):
    def run_main(self, args):
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            status = check_help_text.main(args)
        return status, errors.getvalue().splitlines()

    def test_real_skill_passes_silently(self):
        self.assertEqual(self.run_main([]), (0, []))

    def test_clean_fixture_passes_silently(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "SKILL.md"
            path.write_text(skill_text(), encoding="utf-8")
            self.assertEqual(self.run_main([str(path)]), (0, []))

    def test_planted_defects_fail_with_one_line_each(self):
        planted = skill_text({
            "Topic: overview": topic("About", OVER_LIMIT),
            "Topic: recording": topic("Recording", next_line=""),
            "Topic: setup": topic("Setup", f"{LONG_SENTENCE}\n\n{SEMICOLON_LINE}"),
            "Topic: trouble": topic("Trouble", LINK_LINE),
            "Example: course with a test ahead": example(DASH_LINE),
            "Example: plan in progress": example(OVER_LIMIT),
        })
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "SKILL.md"
            path.write_text(planted, encoding="utf-8")
            status, lines = self.run_main([str(path)])
        self.assertEqual(status, 1)
        self.assertRegex(lines[0], r"^Topic: overview: \d+ words, over the limit of 180$")
        self.assertRegex(lines[-1], r"^Example: plan in progress: \d+ words, over the limit of 180$")
        self.assertEqual(
            lines[1:-1],
            [
                "Topic: recording: needs exactly one Next: line, and it must close the text",
                f"Topic: setup: sentence of 21 words: {LONG_SENTENCE!r}",
                f"Topic: setup: semicolon: {SEMICOLON_LINE!r}",
                f"Topic: setup: abbreviation: {SEMICOLON_LINE!r}",
                f"Topic: trouble: markdown link: {LINK_LINE!r}",
                f"Example: course with a test ahead: long dash: {DASH_LINE!r}",
                f"Example: course with a test ahead: curly quote: {DASH_LINE!r}",
            ],
        )

    def test_a_renamed_topic_is_reported_missing(self):
        text = skill_text().replace("## Topic: trouble\n", "## Topic: troubleshooting\n")
        self.assertEqual(check_help_text.problems(text), ["Topic: trouble: missing"])


if __name__ == "__main__":
    unittest.main()
