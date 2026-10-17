"""Small authored PDF fixtures, generated with standard-library bytes only.

These contain real text objects on two physical pages, or an image-only page.
No Chrome, Docling, models, network sources, or user documents are required.
"""
from pathlib import Path
import zlib

TEXT_PAGES = (
    "WAL readers continue from a stable end mark.",
    "The base read is not conditional. Writers append frames.",
)


def write_pdf(path, pages=TEXT_PAGES, *, image_only=False):
    objects = [b"<< /Type /Catalog /Pages 2 0 R >>", b"", b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"]
    page_ids = []
    if image_only:
        # White pixel as a real image XObject, no hidden text layer.
        image = zlib.compress(b"\xff\xff\xff")
        objects.append(b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length %d >>\nstream\n" % len(image) + image + b"\nendstream")
        pages = (None,)
    for text in pages:
        page_id = len(objects) + 1
        content_id = page_id + 1
        page_ids.append(page_id)
        resources = b"/XObject << /Im0 4 0 R >>" if image_only else b"/Font << /F1 3 0 R >>"
        objects.append(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << " + resources + b" >> /Contents %d 0 R >>" % content_id)
        if image_only:
            content = b"q 100 0 0 100 50 50 cm /Im0 Do Q"
        else:
            escaped = text.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)").encode("ascii")
            content = b"BT /F1 14 Tf 40 740 Td (" + escaped + b") Tj ET"
        objects.append(b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream")
    objects[1] = b"<< /Type /Pages /Kids [" + b" ".join(b"%d 0 R" % number for number in page_ids) + b"] /Count %d >>" % len(page_ids)
    output, offsets = bytearray(b"%PDF-1.4\n"), []
    for number, body in enumerate(objects, 1):
        offsets.append(len(output))
        output += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref = len(output)
    output += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objects) + 1)
    for offset in offsets:
        output += b"%010d 00000 n \n" % offset
    output += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objects) + 1, xref)
    Path(path).write_bytes(output)


def smoke_manifest():
    return {"documents": [
        {"file": "readable.pdf", "kind": "pdf", "anchors": [{"page": index, "text": text} for index, text in enumerate(TEXT_PAGES, 1)],
         "native": {"support": "readable", "coverage": "complete"}},
        {"file": "blank.pdf", "kind": "pdf", "anchors": [],
         "native": {"support": "text_unavailable", "coverage": "text_unavailable", "anchor": {"page": 1}, "query": "unreadable scanned glyphs"}},
    ]}
