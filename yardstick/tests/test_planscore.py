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
        before = count(result, "listed_before_written")
        self.assertEqual((before["value"], before["of"]), (2, 3))
        self.assertEqual([x["pass"] for x in before["items"]], [True, True, False])
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

    def test_a_tutor_written_problem_before_any_listed_exercise(self):
        folder = plan("## Section A\n\n- Drill 1 first\n- Exercise 2.1\n")
        result = count(planscore.score(folder, LISTS), "listed_before_written")
        self.assertEqual(result["items"][0]["first_written"].lower(), "drill")
        self.assertFalse(result["items"][0]["pass"])

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
        self.assertEqual(count(used, "listed_before_written")["value"], 4)
        self.assertEqual(count(used, "added_items_with_practice")["value"], 4)
        self.assertEqual(count(used, "sections_in_lecture_order")["value"], 4)
        self.assertLess(count(own, "slide_ideas_with_practice")["value"], count(used, "slide_ideas_with_practice")["value"])


class Judged(unittest.TestCase):
    def test_judged_counts_come_from_the_answer(self):
        def judge(folder, lines, sections, uncovered, cuts):
            return planscore.read_judged({
                "sections": [{"section": "A", "small_test": True, "quote": "q"}, {"section": "B", "small_test": False, "quote": ""}],
                "carried": [{"idea": "S9", "named": True, "quote": "carried by 2.9"}],
                "cuts": [{"cut": "5, then 4", "named_by_content": False, "quote": "Drop order: 5, then 4."}]})

        result = planscore.score(plan("| A-1 | Exercise 2.1 |\n"), LISTS, judge)
        self.assertEqual((count(result, "sections_small_tests")["value"], count(result, "sections_small_tests")["of"]), (1, 2))
        self.assertEqual(count(result, "carried_ideas_named")["value"], 1)
        self.assertEqual(count(result, "cuts_named")["value"], 0)

    def test_the_prompt_names_sections_and_uncovered_ideas(self):
        prompt = planscore.judge_prompt(plan("text"), LISTS["order"]["sections"], [{"id": "S9", "title": "Bernoulli"}])
        self.assertIn("- C: named distributions", prompt)
        self.assertIn("- S9: Bernoulli", prompt)


class Helpers(unittest.TestCase):
    def test_pages_and_rising(self):
        self.assertEqual(planscore.pages_in("slides 2 to 4 and slide 9"), {2, 3, 4, 9})
        self.assertEqual(planscore.longest_rising([0, 2, 1, 3]), 3)


if __name__ == "__main__":
    unittest.main()
