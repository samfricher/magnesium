#!/usr/bin/env python3
"""Magnesium local document-conversion worker (LEARN-208).

Optional, separately packaged Docling worker speaking the native
`MAGNESIUM_DOCS_V1` framed protocol over stdio (see
`src/retrieval/manifest/convert.rs`). The native runtime opens the
authorized source itself and hands this worker a read-only snapshot
path; the worker accepts no model-supplied paths or URL fetches and
performs no network enrichment.

Contract:
- stdin: one frame `MAGNESIUM_DOCS_V1 <len>\\n<json>` with
  `{version, source_kind, source_sha256, snapshot_path, anchor_start,
  anchor_end}`.
- stdout: one frame `MAGNESIUM_DOCS_V1 <len>\\n<json>` with
  `{version, method, method_version, source_sha256, blocks, warnings,
  coverage, missing?, reason?}`. Blocks carry `anchor_kind`
  (`page`|`section`), 1-based `anchor_start`/`anchor_end`, `text`, and
  an optional `warning` for uncertain reading order, formulas, or table
  relationships. Physical page indices are file-page indices; printed
  page numbers are never substituted.
- Any failure exits nonzero with a bounded stderr line; the native
  side preserves the original and the last valid index generation.

Conversion uses the pinned local Docling pipeline (CPU, remote
enrichment disabled). OCR stays off unless `MAGNESIUM_DOCS_WORKER_OCR=1`
and local OCR models are installed; without OCR, scanned pages report
`text_unavailable` honestly instead of guessing. Extracted text and
confidence values are derived data, never facts about a learner and
never proof of mathematical correctness.

Docling API surface targets docling-slim 2.126.0 /
docling-core 2.96.0 (see README.md and requirements.txt in
this directory).
Executed conversion is NOT_RUN where Docling is not installed; the
framing, validation, and mapping paths are covered by
workers/docling/tests/ with a stubbed docling module.
"""

import hashlib
import json
import os
import sys

PROTOCOL = "MAGNESIUM_DOCS_V1"
VERSION = 1
METHOD = "docling"
SUPPORTED_KINDS = ("pdf", "docx", "pptx", "epub")
MAX_REQUEST_BYTES = 64 * 1024
MAX_RESPONSE_BYTES = 8 * 1024 * 1024
MAX_SNAPSHOT_BYTES = 64 * 1024 * 1024
MAX_TEXT_BYTES = 2 * 1024 * 1024
MAX_BLOCKS = 512

# Labels that never become conversion blocks on their own.
SKIP_LABELS = frozenset(
    {
        "page_header",
        "page_footer",
        "picture",
        "marker",
        "empty_value",
    }
)
# Labels whose text needs an explicit uncertainty warning.
WARN_LABELS = frozenset({"table", "formula"})


def fail(message):
    sys.stderr.write("magnesium-docs-worker: %s\n" % str(message)[:240])
    raise SystemExit(3)


def read_exact(stream, count):
    chunks = []
    remaining = count
    while remaining > 0:
        piece = stream.read(min(remaining, 65536))
        if not piece:
            fail("truncated worker request")
        chunks.append(piece)
        remaining -= len(piece)
    return b"".join(chunks)


def read_request():
    raw = sys.stdin.buffer.readline(MAX_REQUEST_BYTES + 2)
    if not raw or len(raw) > MAX_REQUEST_BYTES + 1:
        fail("missing or oversized request frame")
    try:
        marker, length = raw.decode("ascii").strip().split(" ", 1)
        count = int(length)
    except ValueError:
        fail("malformed request frame")
    if marker != PROTOCOL or count < 2 or count > MAX_REQUEST_BYTES:
        fail("unsupported request frame")
    body = read_exact(sys.stdin.buffer, count)
    try:
        return json.loads(body.decode("utf-8"))
    except ValueError:
        fail("request is not JSON")


def validated_snapshot(path_value):
    if not isinstance(path_value, str) or not path_value or len(path_value) > 4096:
        fail("snapshot path is outside the bound")
    if "\0" in path_value:
        fail("snapshot path is outside the bound")
    if not os.path.isfile(path_value) or os.path.islink(path_value):
        fail("snapshot is not a regular file")
    size = os.path.getsize(path_value)
    if size > MAX_SNAPSHOT_BYTES:
        fail("snapshot exceeds the bound")
    with open(path_value, "rb") as handle:
        digest = hashlib.sha256(handle.read()).hexdigest()
    return path_value, digest


def validate_request(request):
    if not isinstance(request, dict):
        fail("request is not an object")
    if request.get("version") != VERSION:
        fail("unsupported extraction version")
    kind = request.get("source_kind")
    if kind not in SUPPORTED_KINDS:
        fail("unsupported source kind")
    sha = request.get("source_sha256")
    if not isinstance(sha, str) or len(sha) != 64 or any(
        c not in "0123456789abcdef" for c in sha
    ):
        fail("invalid source_sha256")
    start = request.get("anchor_start")
    end = request.get("anchor_end")
    if (
        not isinstance(start, int)
        or not isinstance(end, int)
        or start < 1
        or end < start
    ):
        fail("invalid conversion range")
    snapshot, observed = validated_snapshot(request.get("snapshot_path"))
    if observed != sha:
        fail("snapshot does not hash to the requested source")
    return kind, sha, snapshot, start, end


def package_versions():
    try:
        from importlib import metadata as importlib_metadata
    except ImportError:  # Python < 3.8 fallback, never hit on 3.11+.
        return "docling unknown"
    versions = {}
    for name in ("docling", "docling-slim", "docling-core"):
        try:
            versions[name] = importlib_metadata.version(name)
        except Exception:
            versions[name] = "unknown"
    return "docling %(docling)s (docling-slim %(docling-slim)s, docling-core %(docling-core)s)" % versions


def convert_with_docling(snapshot, kind, start, end):
    try:
        from docling.datamodel.base_models import InputFormat
        from docling.datamodel.settings import PageRange  # noqa: F401 (re-export check)
        from docling.document_converter import DocumentConverter, PdfFormatOption
        from docling.datamodel.pipeline_options import PdfPipelineOptions
    except ImportError as error:
        fail("docling is not installed: %s" % error)
    ocr = os.environ.get("MAGNESIUM_DOCS_WORKER_OCR") == "1"
    try:
        pipeline = PdfPipelineOptions()
        pipeline.do_ocr = ocr
        pipeline.do_table_structure = True
        options = {InputFormat.PDF: PdfFormatOption(pipeline_options=pipeline)}
        converter = DocumentConverter(format_options=options)
        result = converter.convert(
            snapshot, max_file_size=MAX_SNAPSHOT_BYTES, page_range=(start, end)
        )
    except Exception as error:
        fail("docling conversion failed: %s" % error)
    status = str(getattr(result, "status", ""))
    if "failure" in status and "partial" not in status:
        errors = getattr(result, "errors", [])
        fail("docling reported failure: %s" % str(errors)[:200])
    document = getattr(result, "document", None)
    if document is None:
        fail("docling returned no document")
    return document, status


def item_pages(item):
    pages = set()
    for prov in getattr(item, "prov", []) or []:
        page = getattr(prov, "page_no", None)
        if isinstance(page, int) and page >= 1:
            pages.add(page)
    return sorted(pages)


def item_text(item, label):
    if label == "table":
        for method in ("export_to_markdown",):
            candidate = getattr(item, method, None)
            if callable(candidate):
                try:
                    rendered = candidate()
                except Exception:
                    rendered = ""
                if isinstance(rendered, str) and rendered.strip():
                    return rendered
    text = getattr(item, "text", "")
    return text if isinstance(text, str) else ""


def document_to_blocks(document, start, end):
    blocks = []
    warnings = []
    truncated = False
    text_bytes = 0
    try:
        items = document.iterate_items()
    except Exception as error:
        fail("docling iteration failed: %s" % error)
    for item, _level in items:
        label = str(getattr(item, "label", "text") or "text")
        if label in SKIP_LABELS:
            continue
        text = item_text(item, label)
        if not text.strip():
            continue
        pages = item_pages(item)
        if pages and (pages[-1] < start or pages[0] > end):
            continue
        if pages:
            anchor_kind = "page"
            anchor_start = max(start, pages[0])
            anchor_end = min(end, pages[-1])
        else:
            # Section-anchored content (DOCX/PPTX/EPUB flow): link to the
            # requested range start so no block floats without an anchor.
            anchor_kind = "section"
            anchor_start = start
            anchor_end = end
        warning = None
        if label in WARN_LABELS:
            warning = (
                "uncertain %s structure; verify against the original "
                "page view" % label
            )
            if warning not in warnings:
                warnings.append(warning)
        text_bytes += len(text.encode("utf-8"))
        if text_bytes > MAX_TEXT_BYTES or len(blocks) >= MAX_BLOCKS:
            truncated = True
            warnings.append(
                "conversion truncated at worker bounds after %d blocks" % len(blocks)
            )
            break
        blocks.append(
            {
                "anchor_kind": anchor_kind,
                "anchor_start": anchor_start,
                "anchor_end": anchor_end,
                "text": text,
                "warning": warning,
            }
        )
    return blocks, warnings, truncated


def write_response(payload):
    body = json.dumps(payload).encode("utf-8")
    if len(body) > MAX_RESPONSE_BYTES:
        fail("conversion output exceeds the bound")
    sys.stdout.buffer.write(
        ("%s %d\n" % (PROTOCOL, len(body))).encode("ascii")
    )
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()


def main():
    request = read_request()
    kind, sha, snapshot, start, end = validate_request(request)
    document, status = convert_with_docling(snapshot, kind, start, end)
    blocks, warnings, truncated = document_to_blocks(document, start, end)
    missing = []
    if not blocks:
        coverage = "text_unavailable"
        payload_reason = "conversion produced no text for the requested range"
    elif truncated or "partial" in status:
        # Truncation is partial, never complete: the unconverted remainder
        # stays visible as missing ranges instead of vanishing.
        coverage = "partial"
        payload_reason = ""
        if truncated:
            missing.append(
                "unconverted remainder beyond worker bounds "
                "(%d blocks kept)" % len(blocks)
            )
    else:
        coverage = "complete"
        payload_reason = ""
    payload = {
        "version": VERSION,
        "method": METHOD,
        "method_version": package_versions(),
        "source_sha256": sha,
        "blocks": blocks,
        "warnings": warnings,
        "coverage": coverage,
    }
    if coverage == "partial":
        payload["missing"] = missing
    if coverage == "text_unavailable":
        payload["reason"] = payload_reason
    write_response(payload)


if __name__ == "__main__":
    main()
