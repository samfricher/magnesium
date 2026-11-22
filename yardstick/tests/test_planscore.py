import tempfile
import unittest
from pathlib import Path

import planscore
from common import HERE, Fixture

LISTS = {
    "exercises": {"exercises": [{"id": "2.1", "section": "A", "in_scope": True}, {"id": "2.5", "section": "B", "in_scope": True},
                                {"id": "3.1", "section": "C", "in_scope": False}]},
    "slide-ideas": {"ideas": [{"id": "S2", "page": 2, "title": "Expected value", "terms": ["expected value"]},
                              {"id": "S6", "page": 6, "title": "Shortcut", "terms": ["shortcut"]},
                              {"id": "S9", "page": 9, "title": "Bernoulli", "terms": ["Bernoulli"]}]},
    "added-items": {"items": [{"id": "D4", "idea": "Bernoulli mean", "slide_pages": [9], "terms": ["Bernoulli"]}]},
    "order": {"sections": [{"id": "A", "label": "expected value", "slide_pages": [1, 2, 3, 4]},
                           {"id": "B", "label": "variance", "slide_pages": [5, 6, 7, 8]},
                           {"id": "C", "label": "named distributions", "slide_pages": [9, 10]}]},
}


def plan(text: str, drills: str | None = None) -> Path:
    folder = Path(tempfile.mkdtemp())
    (folder / "plan.md").write_text(text, encoding="utf-8")
    if drills:
        (folder / "drills.md").write_text(drills, encoding="utf-8")
    return folder


def count(result, key):
    return next(c for c in result["counts"] if c["key"] == key)


class Mechanical(unittest.TestCase):
    def test_a_plan_in_lecture_order_with_every_listed_exercise(self):
        folder = plan("# Plan\n\n## Section A\n\n| Row | Item |\n|---|---|\n| A-1 | Exercise 2.1, expected value |\n\n"
                      "## Section B\n\n| B-1 | Exercise 2.5, the shortcut |\n| B-2 | Drill 1 |\n\n## Section C\n\n- Drill 2: Bernoulli mean\n")
        result = planscore.score(folder, LISTS)
        self.assertEqual((count(result, "listed_exercises_with_row")["value"], count(result, "listed_exercises_with_row")["of"]), (2, 2))
        self.assertEqual(count(result, "slide_ideas_with_practice")["value"], 3)
        self.assertEqual(count(result, "added_items_with_practice")["value"], 1)
        self.assertEqual(count(result, "sections_in_lecture_order")["value"], 3)
        self.assertEqual(count(result, "listed_exercises_posed")["value"], 2)
        self.assertIsNone(count(result, "cuts_named")["value"])

    def test_numbers_that_are_not_exercises_do_not_count(self):
        folder = plan("| 3 | Teach the shortcut | 60 min | 2.5 h |\n\n1. 0.5 + 0.6 + 1.2 = 2.1\n")
        self.assertEqual(count(planscore.score(folder, LISTS), "listed_exercises_with_row")["value"], 0)

    def test_a_wide_page_range_is_not_a_practice_item_for_each_idea(self):
        folder = plan("| Diagnostic | test | slides 1 to 11 |\n")
        self.assertEqual(count(planscore.score(folder, LISTS), "slide_ideas_with_practice")["value"], 0)

    def test_sections_out_of_order(self):
        folder = plan("## Section C\n\n## Section A\n\n## Section B\n")
        result = count(planscore.score(folder, LISTS), "sections_in_lecture_order")
        self.assertEqual(result["found"], 3)
        self.assertEqual(result["value"], 2)
        self.assertEqual(result["order_seen"], ["C", "A", "B"])

    def test_a_carried_exercise_is_not_posed(self):
        folder = plan("| # | Block | Duration |\n|---|---|---|\n| 1 | Section A: drill 25 (it carries 2.1) | 20 min |\n"
                      "| 2 | Section B: Exercise 2.5 in the book's wording | 10 min |\n")
        result = planscore.score(folder, LISTS)
        posed = {x["id"]: x["posed"] for x in count(result, "listed_exercises_posed")["items"]}
        self.assertIsNone(posed["2.1"])
        self.assertEqual(posed["2.5"], "plan.md:4")
        self.assertEqual(count(result, "listed_exercises_mentioned")["value"], 2)

    def test_a_coverage_row_orders_nothing(self):
        folder = plan("| # | Block | Duration |\n|---|---|---|\n| 1 | slides 1 to 10, every section | 5 min |\n| 2 | Section B | 5 min |\n| 3 | Section A | 5 min |\n")
        self.assertEqual(count(planscore.score(folder, LISTS), "sections_in_lecture_order")["order_seen"], ["B", "A"])

    def test_the_dummy_plans(self):
        fixture = Fixture(HERE / "dummy" / "course")
        own = planscore.score(fixture.plans()["magnesium"], fixture.lists())
        used = planscore.score(fixture.plans()["used"], fixture.lists())
        self.assertEqual(count(own, "listed_exercises_posed")["value"], 0)
        self.assertEqual(count(used, "listed_exercises_posed")["value"], 12)
        self.assertEqual(count(used, "added_items_with_practice")["value"], 4)
        self.assertEqual(count(used, "sections_in_lecture_order")["value"], 4)
        self.assertLess(count(own, "slide_ideas_with_practice")["value"], count(used, "slide_ideas_with_practice")["value"])


class Judged(unittest.TestCase):
    def test_judged_counts_come_from_the_answer(self):
        def judge(folder, lines, sections, uncovered, cuts):
            return planscore.read_judged({
                "carried": [{"idea": "S9", "named": True, "quote": "carried by 2.9"}],
                "cuts": [{"cut": "5, then 4", "named_by_content": False, "quote": "Drop order: 5, then 4."}]})

        result = planscore.score(plan("| A-1 | Exercise 2.1 |\n"), LISTS, judge)
        self.assertEqual(count(result, "carried_ideas_named")["value"], 1)
        self.assertEqual(count(result, "cuts_named")["value"], 0)

    def test_the_prompt_names_uncovered_ideas(self):
        prompt = planscore.judge_prompt(plan("text"), [{"id": "S9", "title": "Bernoulli"}])
        self.assertIn("- S9: Bernoulli", prompt)


def mark(block, kind, sections=(), listed=(), unseen=None, done=False):
    return {"block": block, "done": done, "sections": list(sections), "kind": kind, "listed": list(listed), "unseen": unseen, "quote": ""}


class Blocks(unittest.TestCase):
    def setUp(self):
        self.fixture = Fixture(HERE / "dummy" / "course")
        self.lists = self.fixture.lists()
        self.sections = self.lists["order"]["sections"]
        self.listed = [e["id"] for e in self.lists["exercises"]["exercises"] if e.get("in_scope", True)]

    def key(self, name):
        return [planscore.clean(m, [s["id"] for s in self.sections], self.listed)
                for m in __import__("json").loads((HERE / "dummy" / "course" / "plans-key" / f"{name}.json").read_text())["blocks"]]

    def value(self, shape, key):
        return next(x["value"] for x in shape if x["key"] == key)

    def test_units_are_the_schedule_rows_or_the_section_headings(self):
        own = planscore.units(self.fixture.plans()["magnesium"], self.lists)
        used = planscore.units(self.fixture.plans()["used"], self.lists)
        self.assertEqual([u["block"] for u in own], [str(n) for n in range(1, 9)])
        self.assertEqual(used[0]["block"], "Section A: expected value (slides 1 to 4)")
        self.assertEqual(used[-1]["block"], "Then 1")
        self.assertEqual(len(used), 5)

    def test_the_key_covers_every_unit(self):
        for name, folder in self.fixture.plans().items():
            self.assertEqual([u["block"] for u in planscore.units(folder, self.lists)], [m["block"] for m in self.key(name)], name)

    def test_the_shape_of_the_two_dummy_plans(self):
        own = planscore.shape(self.key("magnesium"), self.sections, self.listed)
        used = planscore.shape(self.key("used"), self.sections, self.listed)
        self.assertEqual(self.value(own, "sections_worked"), 3)
        self.assertEqual(self.value(used, "sections_worked"), 4)
        self.assertEqual(self.value(own, "sections_unseen"), 0)
        self.assertEqual(self.value(used, "sections_unseen"), 4)
        self.assertEqual(self.value(own, "listed_set_or_carried"), 0)
        self.assertEqual(self.value(used, "listed_set_or_carried"), 12)
        self.assertTrue(self.value(own, "tutor_paper_first"))
        self.assertFalse(self.value(used, "tutor_paper_first"))
        self.assertTrue(self.value(used, "instructor_paper"))

    def test_done_blocks_do_not_count(self):
        marks = [mark("1", "tutor_paper", ["all"], done=True), mark("2", "test_first", ["A"], unseen=True)]
        shape = planscore.shape(marks, self.sections, self.listed)
        self.assertFalse(self.value(shape, "tutor_paper_first"))
        self.assertEqual(self.value(shape, "sections_test_first"), 1)

    def test_lecture_order_counts_the_first_block_of_each_section(self):
        marks = [mark("1", "teach_first", ["C"]), mark("2", "teach_first", ["A"]), mark("3", "teach_first", ["B", "C"])]
        shape = planscore.shape(marks, self.sections, self.listed)
        self.assertEqual(self.value(shape, "sections_in_order"), 2)
        self.assertEqual(next(x["order_seen"] for x in shape if x["key"] == "sections_in_order"), ["C", "A", "B"])

    def test_the_majority_of_three_runs(self):
        units = [{"block": "1"}, {"block": "2"}]
        runs = [[mark("1", "teach_first", ["A"], ["2.1"], True), mark("2", "marking")],
                [mark("1", "test_first", ["A", "B"], ["2.1"], True), mark("2", "marking")],
                [mark("1", "teach_first", ["A"], [], False), mark("2", "redo")]]
        voted = planscore.majority(runs, units)
        self.assertEqual((voted[0]["kind"], voted[0]["sections"], voted[0]["listed"], voted[0]["unseen"]), ("teach_first", ["A"], ["2.1"], True))
        self.assertEqual(voted[1]["kind"], "marking")
        spread = planscore.spread_of(runs, units)["identical_across_runs"]
        self.assertEqual((spread["kind"], spread["sections"]), (0, 1))

    def test_marks_are_matched_to_units_by_name(self):
        units = [{"block": "1"}, {"block": "2"}]
        answer = {"blocks": [{"block": "2", "done": False, "sections": [], "kind": "marking", "listed": [], "unseen": None, "quote": ""},
                             {"block": "1", "done": False, "sections": ["A", "Z"], "kind": "nonsense", "listed": ["2.1", "9.9"], "unseen": True, "quote": ""}]}
        marks = planscore.read_marks(answer, units, ["A", "B"], ["2.1"])
        self.assertEqual([m["block"] for m in marks], ["1", "2"])
        self.assertEqual((marks[0]["kind"], marks[0]["sections"], marks[0]["listed"], marks[0]["unseen"]), ("other", ["A"], ["2.1"], None))
        self.assertEqual(marks[1]["kind"], "marking")

    def test_agreement_with_the_key(self):
        key = self.key("magnesium")
        judged = [dict(m) for m in key]
        judged[2] = dict(judged[2], kind="chosen_by_marks", unseen=None)
        result = planscore.agree(key, judged)
        self.assertEqual(result["kind"], [7, 8])
        self.assertEqual((result["sections"], result["unseen"]), ([3, 3], [2, 3]))
        self.assertEqual(result["differ"][0]["block"], "3")

    def test_listed_exercises_are_read_from_setting_and_carrying_clauses(self):
        ids = ["8.27", "8.29", "8.39", "8.41", "8.47", "8.57", "8.58"]
        row = ("| 8 | Section D | C8 slides 49 to 56: 8.57, 8.58. Test: drill 32; drill 11 (it carries 8.27 and 8.29). "
               "Then Exercise 8.47 in the book's wording |")
        self.assertEqual(planscore.set_or_carried(row, ids), ["8.27", "8.29", "8.47"])
        self.assertEqual(planscore.set_or_carried("| 7 | Section C | C8 slides 40 to 48: 8.39, 8.41 |", ids), [])

    def test_unseen_comes_from_the_problems_listed(self):
        seen = planscore.clean({"kind": "teach_first", "problems": [{"problem": "Diagnostic Q2", "seen": True, "helped": False}]}, ["A"], [])
        fresh = planscore.clean({"kind": "test_first", "problems": [{"problem": "Diagnostic Q2", "seen": True, "helped": False},
                                                                     {"problem": "MockA Q3", "seen": False, "helped": False}]}, ["A"], [])
        helped = planscore.clean({"kind": "teach_first", "problems": [{"problem": "drill 4", "seen": False, "helped": True}]}, ["A"], [])
        self.assertEqual((seen["unseen"], fresh["unseen"], helped["unseen"]), (False, True, False))

    def test_the_prompt_lists_every_block(self):
        units = planscore.units(self.fixture.plans()["used"], self.lists)
        prompt = planscore.block_prompt(self.fixture.plans()["used"], self.lists, units)
        self.assertIn("- Then 1: The old test 1", prompt)
        self.assertIn("2.17 (section D)", prompt)
        self.assertIn("A block is one unit of scheduled work", prompt)


class Helpers(unittest.TestCase):
    def test_pages_and_rising(self):
        self.assertEqual(planscore.pages_in("slides 2 to 4 and slide 9"), {2, 3, 4, 9})
        self.assertEqual(planscore.longest_rising([0, 2, 1, 3]), 3)


if __name__ == "__main__":
    unittest.main()
