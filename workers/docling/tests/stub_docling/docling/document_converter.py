"""Stub converter mirroring the pinned docling API surface.

Behavior is driven by `MAGNESIUM_STUB_MODE` (`items`, `partial`, `raise`,
`empty`) and `MAGNESIUM_STUB_ITEMS` (JSON item list); see README.
"""

import json
import os


class PdfFormatOption:
    def __init__(self, pipeline_options=None):
        self.pipeline_options = pipeline_options


class _Prov:
    def __init__(self, page_no):
        self.page_no = page_no


class _Item:
    def __init__(self, label, text, pages):
        self.label = label
        self.text = text
        self.prov = [_Prov(page) for page in pages]

    def export_to_markdown(self):
        if self.label == "table":
            return self.text
        raise AttributeError("no markdown export")


class _Document:
    def __init__(self, items):
        self._items = items

    def iterate_items(self):
        return [(item, 0) for item in self._items]


DEFAULT_ITEMS = [
    {"label": "title", "text": "Stub paper on WAL isolation", "pages": [1]},
    {"label": "text", "text": "Readers pin the end mark.", "pages": [1]},
    {
        "label": "table",
        "text": "| mark | frame |\n|---|---|\n| end | appended |",
        "pages": [2],
    },
    {"label": "formula", "text": "end_mark = max(frames)", "pages": [2]},
]


class _Result:
    def __init__(self, status, document):
        self.status = status
        self.document = document
        self.errors = []


class DocumentConverter:
    def __init__(self, format_options=None):
        self.format_options = format_options or {}

    def convert(self, source, max_file_size=None, page_range=None):
        mode = os.environ.get("MAGNESIUM_STUB_MODE", "items")
        if mode == "raise":
            raise RuntimeError("stub conversion failure")
        if mode == "empty":
            raw = []
        else:
            spec = json.loads(os.environ.get("MAGNESIUM_STUB_ITEMS", "null") or "null")
            raw = DEFAULT_ITEMS if spec is None else spec
        items = [
            _Item(entry["label"], entry["text"], entry.get("pages", [1]))
            for entry in raw
        ]
        status = "partial_success" if mode == "partial" else "success"
        return _Result(status, _Document(items))
