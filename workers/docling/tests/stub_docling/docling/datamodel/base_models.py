"""Stub base models mirroring the pinned docling API surface."""

from enum import Enum


class ConversionStatus(str, Enum):
    PENDING = "pending"
    STARTED = "started"
    FAILURE = "failure"
    SUCCESS = "success"
    PARTIAL_SUCCESS = "partial_success"
    SKIPPED = "skipped"


class InputFormat:
    PDF = "pdf"
    DOCX = "docx"
    PPTX = "pptx"
    EPUB = "epub"
