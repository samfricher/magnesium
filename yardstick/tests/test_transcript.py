import json
import tempfile
import unittest
from pathlib import Path

import transcript
from common import HERE

P = "mcp__plugin_magnesium_magnesium__"


def write(lines):
    handle = tempfile.NamedTemporaryFile("w", suffix=".jsonl", delete=False, encoding="utf-8")
    for line in lines:
        handle.write(json.dumps(line) + "\n")
    handle.close()
    return Path(handle.name)


def user(ts, content, **extra):
    return {"type": "user", "timestamp": ts, "message": {"role": "user", "content": content}, **extra}


def tool(ts, tid, name, payload):
    return {"type": "assistant", "timestamp": ts, "message": {"content": [{"type": "tool_use", "id": tid, "name": name, "input": payload}]}}


def result(ts, tid, text):
    return user(ts, [{"type": "tool_result", "tool_use_id": tid, "content": [{"type": "text", "text": text}]}])


class TurnCost(unittest.TestCase):
    def test_counts_learner_messages_calls_and_characters_per_day(self):
        marked = {"observations": [{"attempt": {"label": "x"}}]}
        path = write([
            user("2026-11-18T15:00:00Z", "first answer"),
            user("2026-11-18T15:00:01Z", "hook text", isMeta=True),
            tool("2026-11-18T15:00:02Z", "a", P + "learning_step", marked),
            tool("2026-11-18T15:00:02Z", "a", P + "learning_step", marked),
            result("2026-11-18T15:00:03Z", "a", json.dumps({"k": "v" * 50})),
            tool("2026-11-18T15:00:04Z", "b", P + "reply", {"text": "Next task?"}),
            result("2026-11-18T15:00:05Z", "b", "{}"),
            tool("2026-11-18T15:00:06Z", "c", "Read", {"file_path": "x"}),
            result("2026-11-18T15:00:07Z", "c", "file text"),
            user("2026-11-18T15:01:00Z", [{"type": "text", "text": "second message"}]),
            user("2026-11-18T15:01:01Z", "summary", isCompactSummary=True),
        ])
        day = transcript.turn_cost(path)["2026-11-18"]
        self.assertEqual(day["learner_messages"], 2)
        self.assertEqual(day["runtime_calls"], 2)
        self.assertEqual(day["record_calls"], 1)
        self.assertEqual(day["record_calls_per_message"], 0.5)
        self.assertEqual(day["marked_calls"], 1)
        self.assertEqual(day["chars_per_marked_call"]["median"], len(json.dumps({"k": "v" * 50})))
        self.assertGreater(day["chars_per_marked_call_pretty"]["median"], day["chars_per_marked_call"]["median"])

    def test_the_dummy_transcript_reads_as_two_days(self):
        path = HERE / "dummy" / "course" / "transcripts" / "lesson.jsonl"
        days = transcript.turn_cost(path)
        self.assertEqual(sorted(days), ["2026-11-19", "2026-11-20"])
        for day in days.values():
            self.assertEqual(day["learner_messages"], 2)
            self.assertEqual(day["record_calls"], 3)
            self.assertEqual(day["marked_calls"], 1)

    def test_an_error_result_is_left_out_of_marked_sizes(self):
        path = write([
            user("2026-11-18T15:00:00Z", "answer"),
            tool("2026-11-18T15:00:02Z", "a", P + "learning_step", {"observations": [{"attempt": {}}]}),
            user("2026-11-18T15:00:03Z", [{"type": "tool_result", "tool_use_id": "a", "is_error": True, "content": "rejected"}]),
        ])
        day = transcript.turn_cost(path)["2026-11-18"]
        self.assertEqual(day["marked_calls"], 0)
        self.assertEqual(day["chars_per_call"]["count"], 1)


class RealUseCounts(unittest.TestCase):
    def test_explanation_requests_stalls_and_replies_without_a_task(self):
        sections = [{"id": "A", "label": "expected value", "terms": ["expected value"]},
                    {"id": "B", "label": "variance", "terms": ["variance"]}]
        path = write([
            user("2026-11-18T15:00:00Z", "/magnesium:learn variance"),
            tool("2026-11-18T15:00:01Z", "r1", P + "reply", {"text": "The variance measures spread. Find Var(X) for X = 0 or 2?"}),
            user("2026-11-18T15:01:00Z", "I don't get it, can you explain?"),
            tool("2026-11-18T15:01:01Z", "r2", P + "reply", {"text": "Let's go back to expected value first: recall that E[X] is a weighted mean."}),
            user("2026-11-18T15:02:00Z", "no idea"),
            tool("2026-11-18T15:02:01Z", "r3", P + "reply", {"text": "Here is the variance worked out in full."}),
        ])
        counts = transcript.real_use_counts(path, sections)
        self.assertEqual(counts["explanation_requests"], {"variance": 1})
        self.assertEqual(counts["stalls"], {"variance": 1})
        self.assertEqual(counts["teaching_replies"], 3)
        self.assertEqual(counts["teaching_replies_setting_no_task"], 2)
        self.assertEqual(counts["detours_to_earlier_step"], {"variance": 1})
        self.assertEqual(counts["planned_items_against_parts_marked"], "no data")

    def test_sets_task(self):
        self.assertTrue(transcript.sets_task("Explanation.\n\nFind Var(2X + 5)."))
        self.assertTrue(transcript.sets_task("What is E[X]?"))
        self.assertFalse(transcript.sets_task("The answer is 2.25, from 4.5 minus 2.25."))


if __name__ == "__main__":
    unittest.main()
