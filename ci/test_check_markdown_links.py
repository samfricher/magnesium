"""The link check reads tracked Markdown only and reports each missing target."""
import contextlib
import io
from pathlib import Path
import subprocess
import tempfile
import unittest

from ci import check_markdown_links


class MarkdownLinkTests(unittest.TestCase):
    def run_main(self, root):
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            status = check_markdown_links.main(root)
        return status, errors.getvalue().splitlines()

    def test_broken_link_in_tracked_file_is_reported_and_untracked_file_is_ignored(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            (root / "good.md").write_text("See [here](other.md) and [web](https://example.com).\n", encoding="utf-8")
            (root / "other.md").write_text("# Other\n", encoding="utf-8")
            (root / "notes.md").write_text("# Notes\n\nSee [gone](missing.md#top).\n", encoding="utf-8")
            subprocess.run(["git", "add", "good.md", "other.md", "notes.md"], cwd=root, check=True)
            (root / "private.md").write_text("[gone](nowhere.md)\n", encoding="utf-8")
            status, lines = self.run_main(root)
        self.assertEqual((status, lines), (1, ["notes.md:3: missing missing.md"]))

    def test_tracked_files_with_working_links_pass(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            (root / "a.md").write_text("[b](b.md)\n", encoding="utf-8")
            (root / "b.md").write_text("# B\n", encoding="utf-8")
            subprocess.run(["git", "add", "a.md", "b.md"], cwd=root, check=True)
            self.assertEqual(self.run_main(root), (0, []))


if __name__ == "__main__":
    unittest.main()
