"""The brand check passes the real tree, flags each kind of old name and spares data formats."""
import contextlib
import io
from pathlib import Path
import subprocess
import tempfile
import unittest

from ci import check_brand

PLANTED = [
    ("Run `learn init` in the folder.", "old command"),
    ("It prints `learn 0.2.0`.", "old command"),
    ('{"enabledPlugins":{"learn@learn":true}}', "old plugin id"),
    ("codex plugin add codex-plugin@magnesium", "old plugin id"),
    ("Type /learn:help to start.", "old namespace"),
    ('"Skill(learn:papers)"', "old namespace"),
    ("<!-- learn:begin -->", "old namespace"),
    ('"mcp__plugin_learn_learn__*"', "old MCP rule"),
    ("Your history lives in `.learn/state.sqlite`.", "old control folder"),
    ('workspace.joinpath(".learn")', "old control folder"),
    ("Exports carry a `.learn-export` marker.", "old control folder"),
    ("Write one name per line in `.magnesium/learnignore`.", "old ignore file"),
    ("ExclusionReason::Learnignore", "old ignore file"),
    ("export LEARN_TEST_BINARY=/tmp/x", "old environment prefix"),
    ('const MAGNESIUM_BIN = "__LEARN_BINARY__";', "old environment prefix"),
    ("https://github.com/samfricher/Learn/releases", "old repository"),
    ("git clone https://github.com/samfricher/learn", "old repository"),
    ('ClapCommand::new("learn")', "old name literal"),
    ("Run `learn` from the study folder.", "old name literal"),
    ("Then install learn from source.", "old command"),
    ("cargo install learn --locked", "old command"),
    ("See learn(1) for the flags.", "old command"),
    ('"mcpServers": {"learn": {', "old name literal"),
    ("include_str!(\"../../opencode-plugin/learn.js\")", "old file name"),
    ("Read LEARN.md before the workflows.", "old file name"),
    ("dist/learn-0.2.0-aarch64-apple-darwin.tar.gz", "old name prefix"),
    ('"serializer":"learn-export-v2"', "old name prefix"),
    ('binary="target/release/learn"', "old path segment"),
    ('"ui://learn/lesson"', "old path segment"),
    ("pub(crate) struct LearnServer {", "old identifier"),
    ("await runLearnHook(client, \"stop\", {});", "old identifier"),
    ("function learnBinary() {", "old identifier"),
    ("fn is_learn_codex_plugin(plugin: &Value) -> bool {", "old identifier"),
    ("Self-gating hooks backed by the native learn runtime.", "old product noun"),
    ("Install the learn CLI first.", "old product noun"),
    ("Learn keeps your learning history.", "old product name"),
    ("Approve the Learn hooks if Codex asks.", "old product name"),
    ('"host_thread_id": "learn-h1:claude:conversation"', "old name prefix"),
    ('digest.update(b"learn-instruction-bundle\\0");', "old name prefix"),
    ('const DOMAIN: &[u8] = b"learn/native-initial-capture/v2\\0";', "old name domain"),
]

ALLOWED = [
    '{"schema":"learner-model/v2","action":"update"}',
    "Call learning_step with the learner's own words.",
    "Type `/magnesium:learn quiz me on cells`.",
    '"Skill(magnesium:learn)"',
    'include_str!("../../claude-plugin/skills/learn/SKILL.md")',
    '&config_dir.join("skills").join("learn").join("SKILL.md"),',
    '"goal": "Learn coefficient roles"',
    '2. **Learn one week.** Say "Teach me week 6 from the slides".',
    "- **Learn from your materials.** The tutor searches your notes.",
    "Ask to learn, practise, or review when you want Magnesium involved.",
    "Load the learn skill and start or resume a session.",
    "Structural acceptance for the synthetic LEARN-257 learning-loop cases.",
]


class BrandCheckTests(unittest.TestCase):
    def run_main(self, args):
        errors = io.StringIO()
        with contextlib.redirect_stderr(errors):
            status = check_brand.main(args)
        return status, errors.getvalue().splitlines()

    def test_real_tree_passes_silently(self):
        self.assertEqual(self.run_main([]), (0, []))

    def test_each_kind_of_old_name_is_flagged(self):
        for line, kind in PLANTED:
            with self.subTest(line=line):
                self.assertIn(kind, {found for _, found, _ in check_brand.leftovers(line)})

    def test_data_formats_skill_names_and_the_verb_pass(self):
        for line in ALLOWED:
            with self.subTest(line=line):
                self.assertEqual(check_brand.leftovers(line), [])

    def test_a_tracked_leftover_fails_with_its_location(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True)
            (root / "notes.md").write_text("# Notes\n\nRun `learn init` here.\n", encoding="utf-8")
            (root / "image.bin").write_bytes(b"\0.learn/state.sqlite")
            subprocess.run(["git", "add", "notes.md", "image.bin"], cwd=root, check=True)
            (root / "untracked.md").write_text("Learn keeps this.\n", encoding="utf-8")
            status, lines = self.run_main(["--root", str(root)])
        self.assertEqual((status, lines), (1, ["notes.md:3: old command: 'learn init'"]))


if __name__ == "__main__":
    unittest.main()
