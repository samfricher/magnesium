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


def plant(text: str, old: str, new: str) -> str:
    assert old in text, f"anchor not in SKILL.md: {old!r}"
    return text.replace(old, new, 1)


def planted_skill() -> str:
    text = check_help_text.SKILL.read_text(encoding="utf-8")
    text = plant(text, "Each study folder keeps its own learning history.\n", "Each study folder keeps its own learning history.\n" + "Magnesium helps you study. " * 15 + "\n")
    text = plant(text, "\nNext: type `/magnesium:help papers` to start practice papers.\n", "\n")
    text = plant(text, "Your study files stay where they are.", LONG_SENTENCE)
    text = plant(text, "The tutor asks before it installs tectonic.", SEMICOLON_LINE)
    text = plant(text, "Ask your coding agent to put `magnesium` on your PATH. Step 4 of docs/install.md shows how.", LINK_LINE)
    text = plant(text, '2. **Learn one week.** Say "Teach me week 6 from the slides".', DASH_LINE)
    return plant(text, "3. **Fit today to your time.**", "Week one is cells. " * 20 + "\n3. **Fit today to your time.**")


class HelpTextCheckTests(unittest.TestCase):
    def run_main(self, args):
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            status = check_help_text.main(args)
        return status, errors.getvalue().splitlines()

    def test_real_skill_passes_silently(self):
        self.assertEqual(self.run_main([]), (0, []))

    def test_planted_defects_fail_with_one_line_each(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "SKILL.md"
            path.write_text(planted_skill(), encoding="utf-8")
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
        text = plant(check_help_text.SKILL.read_text(encoding="utf-8"), "## Topic: trouble", "## Topic: troubleshooting")
        self.assertEqual(check_help_text.problems(text), ["Topic: trouble: missing"])


if __name__ == "__main__":
    unittest.main()
