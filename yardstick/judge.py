from __future__ import annotations

import os
import shutil
import threading
from pathlib import Path

import claude_code
from adapter import sign_in, sign_out
from claude_code import add_usage

VERDICT = {
    "type": "object",
    "properties": {"pass": {"type": "boolean"}, "sentence": {"type": "string"}, "why": {"type": "string"}},
    "required": ["pass", "sentence", "why"],
}


class Judge:
    def __init__(self, work: Path, model: str, effort: str):
        self.model, self.effort = model, effort
        self.root = Path(work) / f"judge-{os.getpid()}"
        if self.root.exists():
            shutil.rmtree(self.root)
        self.home = self.root / "home"
        self.tmp = self.root / "tmp"
        self.tmp.mkdir(parents=True)
        sign_in(self.home)
        self.lock = threading.Lock()
        self.usage: dict = {}
        self.calls = 0
        self.seconds = 0.0

    def env(self) -> dict:
        return {"HOME": str(self.home), "USER": os.environ.get("USER", ""), "LOGNAME": os.environ.get("LOGNAME", ""),
                "PATH": "/usr/bin:/bin", "TMPDIR": str(self.tmp)}

    def ask(self, prompt: str, schema: dict = VERDICT) -> dict:
        result = claude_code.ask(prompt, self.env(), self.root, self.model, self.effort, schema)
        with self.lock:
            self.calls += 1
            self.seconds += result["seconds"]
            add_usage(self.usage, result["usage"])
        return result["answer"] or {}

    def verdict(self, rubric: str, exchange: str) -> dict:
        prompt = (
            "You judge one tutoring exchange against a fixed rubric. Answer only from the exchange. "
            "Return pass true or false, the one sentence from the tutor's words that your verdict rests on, copied exactly, "
            "and a short reason.\n\nRUBRIC\n" + rubric + "\n\nEXCHANGE\n" + exchange
        )
        answer = self.ask(prompt)
        return {"pass": bool(answer.get("pass")), "sentence": answer.get("sentence", ""), "why": answer.get("why", "")}

    def close(self):
        sign_out(self.home)

    def cost(self) -> dict:
        return {"calls": self.calls, "seconds": round(self.seconds, 1), "usage": self.usage}
