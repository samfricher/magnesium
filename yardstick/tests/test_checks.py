import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

import adapter
import check_coverage
import check_evidence
import check_scenes
from common import HERE, Fixture, mentions, spread, table


class Evidence(unittest.TestCase):
    def test_matches_label_or_kind(self):
        self.assertTrue(check_evidence.matches({"label": "Variance shortcut", "kind": None}, ["variance shortcut"]))
        self.assertTrue(check_evidence.matches({"label": "x", "kind": "VARSHORT"}, ["varshort"]))
        self.assertFalse(check_evidence.matches({"label": "x", "kind": None}, ["y"]))

    def test_labels_read_the_runtime_words(self):
        progress = {"other": {"skills": [{"skill": "variance shortcut", "passes": 1, "due": True, "reading": None}]},
                    "plans": [{"skills": [{"skill": "VARSHORT", "passes": 0, "reading": "open", "next_on": "2026-11-22"}]}]}
        self.assertEqual(check_evidence.labels_from(progress, ["variance shortcut", "VARSHORT"]),
                         ["variance shortcut: 1 of 3 passes, due", "VARSHORT: open, 0 of 3 passes, next 2026-11-22"])

    def test_the_dummy_record_shows_the_warning(self):
        fixture = Fixture(HERE / "dummy" / "course")
        attempts = fixture.attempts()
        item = next(i for i in fixture.asked()["items"] if i["id"] == "Q1b")
        before = [a for a in attempts if check_evidence.matches(a, item["record_tags"]) and a["answered_at"] < fixture.test_at]
        clean = [a for a in before if a["mark"] == "right" and a["help"] == "none" and a["cold"]]
        self.assertEqual((len(before), len(clean)), (5, 0))


class Coverage(unittest.TestCase):
    def test_in_material(self):
        fixture = Fixture(HERE / "dummy" / "course")
        found, _ = check_coverage.in_material(fixture, [{"path": "Materials/Slides/unit-2-slides.md", "where": "Slide 13"}], [])
        self.assertEqual(found, "yes")
        found, where = check_coverage.in_material(fixture, [{"path": "Materials/none.md", "where": "x"}], [])
        self.assertEqual(found, "no")
        self.assertIn("missing: Materials/none.md", where)


class FakeAdapter:
    def posed_check(self, reply):
        return "quick check 1" in reply.lower()

    def is_runtime_tool(self, tool):
        return tool.startswith("mcp__")

    def is_reply_tool(self, tool):
        return tool.endswith("__reply")


def turn(tutor, task=None, tools=()):
    state = {"lesson": {"open_task": task}} if task is not False else {"lesson": None}
    return {"learner": "l", "tutor": tutor, "tools": list(tools), "state": state}


class Scenes(unittest.TestCase):
    def test_mechanical_rules(self):
        a = FakeAdapter()
        turns = [turn("Var is 2.25.", None)]
        self.assertTrue(check_scenes.decide({"kind": "contains", "turn": 1, "any": ["2.25"]}, turns, a, None)["pass"])
        self.assertTrue(check_scenes.decide({"kind": "no_open_task"}, turns, a, None)["pass"])
        kept = [turn("Task?", {"text": "T", "posed_at_ms": 1}), turn("Side answer. T still waits.", {"text": "T", "posed_at_ms": 1})]
        self.assertTrue(check_scenes.decide({"kind": "open_task_kept", "from": 1, "to": 2}, kept, a, None)["pass"])
        dropped = [kept[0], turn("Side answer.", None)]
        self.assertFalse(check_scenes.decide({"kind": "open_task_kept", "from": 1, "to": 2}, dropped, a, None)["pass"])
        check = [turn("Quick check 1: a machine jams with probability 0.2.", {"kind": "task", "text": "x"})]
        self.assertTrue(check_scenes.decide({"kind": "posed_check", "turn": 1, "words": [["0.2", "jam"]]}, check, a, None)["pass"])
        new = [turn("A die is rolled once.", {"kind": "task", "text": "x"})]
        self.assertFalse(check_scenes.decide({"kind": "posed_check", "turn": 1, "words": [["0.2", "jam"]], "labels": ["geometric"]}, new, a, None)["pass"])

    def test_tool_calls_before_the_reply(self):
        tools = ["mcp__m__context", "Read", "mcp__m__learning_step", "mcp__m__reply", "mcp__m__learning_step"]
        out = check_scenes.decide({"kind": "tool_calls", "turn": 1}, [turn("x", None, tools)], FakeAdapter(), None)
        self.assertEqual((out["tool_calls"], out["runtime_calls"], out["turn_calls"]), (3, 2, 5))
        self.assertIsNone(out["pass"])
        sized = dict(turn("x", None, tools), tool_chars=[100, 2000, 300, 5, 5])
        capped = check_scenes.decide({"kind": "tool_calls", "turn": 1, "most": 0}, [sized], FakeAdapter(), None)
        self.assertFalse(capped["pass"])
        self.assertEqual(capped["chars_before_reply"], 2400)
        none = check_scenes.decide({"kind": "tool_calls", "turn": 1, "most": 0}, [turn("x", None, ["mcp__m__reply"])], FakeAdapter(), None)
        self.assertTrue(none["pass"])

    def test_the_test_date_moves(self):
        folder = Path(tempfile.mkdtemp())
        (folder / "Study" / "t").mkdir(parents=True)
        (folder / "Study" / "t" / "plan.md").write_text("| Item | Value |\n|---|---|\n| When | 2020-01-06 (Mon) 10:00 |\n")
        check_scenes.set_test_date(SimpleNamespace(folder=folder), "Study/t", 3)
        text = (folder / "Study" / "t" / "plan.md").read_text()
        self.assertNotIn("2020-01-06", text)
        self.assertIn("| When |", text)

    def test_every_scene_has_rules(self):
        spec = __import__("json").loads((HERE / "dummy" / "course" / "scenes.json").read_text())
        self.assertEqual(len(spec["scenes"]), 7)
        for scene in spec["scenes"]:
            self.assertTrue(scene["messages"] and scene["decide"] and scene["from"], scene["id"])
        self.assertEqual({s["id"] for s in spec["scenes"] if s.get("guard")}, {"side-question", "due-checks"})


class SignIn(unittest.TestCase):
    def test_a_run_leaves_no_keychain_link(self):
        home = Path(tempfile.mkdtemp()) / "home"
        released = []
        fake = SimpleNamespace(place=lambda *a, **k: SimpleNamespace(home=home), release=lambda place: released.append(place))
        adapter.sign_in(home)
        self.assertTrue((home / "Library" / "Keychains").is_symlink())
        with self.assertRaises(AttributeError):
            check_scenes.one_run(SimpleNamespace(), fake, SimpleNamespace(folder=None), {"id": "x"}, 1, None, [])
        self.assertEqual(len(released), 1)
        adapter.sign_out(home)
        self.assertFalse((home / "Library" / "Keychains").exists())
        self.assertTrue((adapter.real_home() / "Library" / "Keychains").is_dir())


class Common(unittest.TestCase):
    def test_mentions_keeps_numbers_whole(self):
        self.assertTrue(mentions("Exercise 2.1, the raffle", "2.1"))
        self.assertFalse(mentions("Exercise 2.15", "2.1"))
        self.assertFalse(mentions("12.5 hours", "2.5"))

    def test_spread_and_table(self):
        self.assertEqual(spread([5, 1, 3])["median"], 3)
        self.assertEqual(spread([])["count"], 0)
        self.assertEqual(table(["Day", "Calls"], [["mon", 2]]).splitlines()[1], "---  -----")


if __name__ == "__main__":
    unittest.main()
