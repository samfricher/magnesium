#!/usr/bin/env python3
"""Generate the fixed 12-document fidelity sample (LEARN-208).

Every file is generated from authored sources with independently known
text and locations (recorded in manifest.json); nothing is downloaded.
PDFs render through headless Chrome print-to-pdf except the scanned
page, which rasterizes real glyph pixels (Chrome screenshot decoded
with stdlib zlib, re-encoded as a PDF image with no text layer).
DOCX/PPTX/EPUB are hand-rolled minimal Office/OpenDocument packages.

Usage: python3 workers/docling/sample/generate.py [--chrome PATH] OUT_DIR
"""

import argparse
import os
import struct
import subprocess
import sys
import tempfile
import zipfile
import zlib

CHROME_CANDIDATES = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
]

WAL_TITLE = "WAL snapshot isolation"
WAL_BODY = (
    "WAL readers continue from a stable end mark while a writer appends frames. "
    "Retention windows depend on checkpoint progress."
)


def find_chrome(explicit=None):
    if explicit and os.path.isfile(explicit):
        return explicit
    for candidate in CHROME_CANDIDATES:
        if os.path.isfile(candidate):
            return candidate
    raise SystemExit("no headless Chrome found; pass --chrome PATH")


def print_pdf(chrome, html, out_path, columns=False):
    with tempfile.TemporaryDirectory(prefix="magnesium-docling-sample-") as staged:
        page = os.path.join(staged, "page.html")
        with open(page, "w") as handle:
            handle.write(html)
        completed = subprocess.run(
            [
                chrome, "--headless=new", "--disable-gpu",
                f"--user-data-dir={os.path.join(staged, 'profile')}",
                "--no-pdf-header-footer", f"--print-to-pdf={out_path}", page,
            ],
            capture_output=True, text=True, timeout=120,
        )
        if completed.returncode != 0 or not os.path.isfile(out_path):
            raise SystemExit(f"print-to-pdf failed: {completed.stderr[-500:]}")


def screenshot_png(chrome, html, out_path, width=1000, height=1250):
    with tempfile.TemporaryDirectory(prefix="magnesium-docling-raster-") as staged:
        page = os.path.join(staged, "page.html")
        with open(page, "w") as handle:
            handle.write(html)
        completed = subprocess.run(
            [chrome, "--headless=new", "--disable-gpu",
             f"--user-data-dir={os.path.join(staged, 'profile')}",
             f"--window-size={width},{height}", "--force-device-scale-factor=1",
             f"--screenshot={out_path}", page],
            capture_output=True, text=True, timeout=120,
        )
        if completed.returncode != 0 or not os.path.isfile(out_path):
            raise SystemExit(f"screenshot failed: {completed.stderr[-500:]}")


def decode_png_8bit_rgb(path):
    """Decode a non-interlaced 8-bit RGB PNG with stdlib only."""
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, width, height, idat = 8, None, None, bytearray()
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, _compression, _filter, interlace = struct.unpack(
                ">IIBBBBB", body
            )
            assert (depth, color, interlace) == (8, 2, 0), "need 8-bit RGB non-interlaced"
        elif kind == b"IDAT":
            idat += body
        elif kind == b"IEND":
            break
        pos += 12 + length
    raw = zlib.decompress(bytes(idat))
    stride = width * 3
    rows, previous, offset = [], bytearray(stride), 0
    for _ in range(height):
        kind = raw[offset]
        offset += 1
        scanline = bytearray(raw[offset : offset + stride])
        offset += stride
        if kind == 1:
            for i in range(3, stride):
                scanline[i] = (scanline[i] + scanline[i - 3]) & 0xFF
        elif kind == 2:
            for i in range(stride):
                scanline[i] = (scanline[i] + previous[i]) & 0xFF
        elif kind == 3:
            for i in range(stride):
                left = scanline[i - 3] if i >= 3 else 0
                scanline[i] = (scanline[i] + ((left + previous[i]) >> 1)) & 0xFF
        elif kind == 4:
            for i in range(stride):
                left = scanline[i - 3] if i >= 3 else 0
                up = previous[i]
                upper_left = previous[i - 3] if i >= 3 else 0
                predictor = left + up - upper_left
                pa, pb, pc = abs(predictor - left), abs(predictor - up), abs(
                    predictor - upper_left
                )
                pick = left if (pa <= pb and pa <= pc) else (up if pb <= pc else upper_left)
                scanline[i] = (scanline[i] + pick) & 0xFF
        elif kind != 0:
            raise SystemExit(f"unsupported PNG filter {kind}")
        rows.append(bytes(scanline))
        previous = scanline
    return width, height, b"".join(rows)


def scanned_pdf_from_text(chrome, lines, out_path):
    """Render text, screenshot it, and embed the pixels with no text layer."""
    words = " ".join(lines)
    html = (
        "<html><body style='margin:120px;font:48px monospace;background:white;color:black'>"
        + "<p>" + "</p><p>".join(lines) + "</p></body></html>"
    )
    shot = out_path + ".shot.png"
    screenshot_png(chrome, html, shot)
    try:
        width, height, rgb = decode_png_8bit_rgb(shot)
    finally:
        if os.path.exists(shot):
            os.unlink(shot)
    # Downscale 2x by pixel-picking to keep the fixture small.
    width //= 2
    height //= 2
    stride = width * 3
    small = bytearray()
    full_stride = width * 2 * 3
    for row in range(height):
        base = (row * 2) * full_stride
        for col in range(width):
            small += rgb[base + col * 6 : base + col * 6 + 3]
    assert len(small) == stride * height
    compressed = zlib.compress(bytes(small), 9)
    image_id, content_id = 4, 5
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] "
        b"/Resources << /XObject << /Im0 %d 0 R >> >> /Contents %d 0 R >>"
        % (width, height, image_id, content_id),
        b"<< /Type /XObject /Subtype /Image /Width %d /Height %d "
        b"/ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode "
        b"/Length %d >>\nstream\n" % (width, height, len(compressed))
        + compressed + b"\nendstream",
        b"<< /Length %d >>\nstream\nq %d 0 0 %d 0 0 cm /Im0 Do Q\nendstream"
        % (30 + len(str(width)) + len(str(height)), width, height),
    ]
    out = bytearray(b"%PDF-1.4\n")
    offsets = []
    for number, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % number + body + b"\nendobj\n"
    xref_at = len(out)
    out += b"xref\n0 6\n0000000000 65535 f \n"
    for offset in offsets:
        out += b"%010d 00000 n \n" % offset
    out += b"trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF" % xref_at
    with open(out_path, "wb") as handle:
        handle.write(bytes(out))
    return words


def write_zip(path, entries, stored_first=None):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as archive:
        if stored_first:
            name, data = stored_first
            info = zipfile.ZipInfo(name)
            info.compress_type = zipfile.ZIP_STORED
            archive.writestr(info, data)
        for name, data in entries:
            archive.writestr(name, data)


def minimal_docx(paragraphs, tables=()):
    def para(text):
        return (
            '<w:p><w:r><w:rPr/><w:t xml:space="preserve">%s</w:t></w:r></w:p>' % text
        )

    def table(rows):
        grid = "".join('<w:gridCol w:w="3000"/>' for _ in rows[0])
        cells = "".join(
            "<w:tr>" + "".join(
                '<w:tc><w:p><w:r><w:t xml:space="preserve">%s</w:t></w:r></w:p></w:tc>' % cell
                for cell in row
            ) + "</w:tr>"
            for row in rows
        )
        return "<w:tbl><w:tblGrid>%s</w:tblGrid>%s</w:tbl>" % (grid, cells)

    body = "".join(para(p) for p in paragraphs) + "".join(table(t) for t in tables)
    document = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
        "<w:body>%s</w:body></w:document>" % body
    )
    return [
        ("[Content_Types].xml",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
         '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
         '<Default Extension="xml" ContentType="application/xml"/>'
         '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
         '<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>'
         "</Types>"),
        ("_rels/.rels",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
         '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
         '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/>'
         "</Relationships>"),
        ("word/document.xml", document),
        ("docProps/core.xml",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"><cp:title>Magnesium sample</cp:title></cp:coreProperties>'),
    ]


def minimal_pptx(slides):
    def slide(texts, slide_id, layout_id="2"):
        shapes = "".join(
            '<p:cxnSp><p:nvCxnSpPr><p:cNvPr id="%d" name="t%d"/>'
            '<p:cNvCxnSpPr/><p:nvPr/></p:nvCxnSpPr><p:spPr/>'
            '<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>%s</a:t></a:r></a:p></p:txBody></p:cxnSp>'
            % (100 + i, i, text)
            for i, text in enumerate(texts)
        )
        return (
            '<?xml version="1.0" encoding="UTF-8"?>'
            '<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" '
            'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">'
            "<p:cSld><p:spTree>%s</p:spTree></p:cSld></p:sld>" % shapes
        )

    parts = [
        ("[Content_Types].xml",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
         '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
         '<Default Extension="xml" ContentType="application/xml"/>'
         '<Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>'
         + "".join(
             '<Override PartName="/ppt/slides/slide%d.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>'
             % (i + 1)
             for i in range(len(slides))
         )
         + "</Types>"),
        ("_rels/.rels",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
         '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/>'
         "</Relationships>"),
        ("ppt/presentation.xml",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">'
         "<p:sldIdLst>"
         + "".join('<p:sldId id="%d" r:id="rId%d"/>' % (256 + i, i + 1) for i in range(len(slides)))
         + "</p:sldIdLst></p:presentation>"),
        ("ppt/_rels/presentation.xml.rels",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
         + "".join(
             '<Relationship Id="rId%d" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide%d.xml"/>'
             % (i + 1, i + 1)
             for i in range(len(slides))
         )
         + "</Relationships>"),
    ]
    for i, texts in enumerate(slides):
        parts.append((f"ppt/slides/slide{i + 1}.xml", slide(texts, i + 1)))
        parts.append((f"ppt/slides/_rels/slide{i + 1}.xml.rels",
                      '<?xml version="1.0" encoding="UTF-8"?>'
                      '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>'))
    return parts


def minimal_epub(title, chapters):
    items = "".join(
        '<item id="ch%d" href="ch%d.xhtml" media-type="application/xhtml+xml"/>' % (i + 1, i + 1)
        for i in range(len(chapters))
    )
    spine = "".join('<itemref idref="ch%d"/>' % (i + 1) for i in range(len(chapters)))
    opf = (
        '<?xml version="1.0" encoding="UTF-8"?>'
        '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">'
        "<metadata><dc:title xmlns:dc='http://purl.org/dc/elements/1.1/'>%s</dc:title></metadata>"
        "<manifest>%s</manifest><spine>%s</spine></package>" % (title, items, spine)
    )
    parts = [
        ("META-INF/container.xml",
         '<?xml version="1.0" encoding="UTF-8"?>'
         '<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">'
         '<rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>'
         "</rootfiles></container>"),
        ("OEBPS/content.opf", opf),
    ]
    for i, (heading, body) in enumerate(chapters):
        parts.append((f"OEBPS/ch{i + 1}.xhtml",
                      '<?xml version="1.0" encoding="UTF-8"?>'
                      '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>%s</title></head>'
                      "<body><h1>%s</h1><p>%s</p></body></html>" % (heading, heading, body)))
    return parts


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chrome")
    parser.add_argument("out")
    args = parser.parse_args(argv)
    chrome = find_chrome(args.chrome)
    out = os.path.abspath(args.out)
    os.makedirs(out, exist_ok=True)
    manifest = []

    def pdf(name, html, anchors):
        path = os.path.join(out, name)
        print_pdf(chrome, html, path)
        manifest.append({"file": name, "kind": "pdf", "anchors": anchors,
                         "native": {"support": "readable", "coverage": "complete"}})

    base_css = "body{font:16px/1.5 serif;margin:72px;color:#111}"
    pdf("ordinary-text.pdf",
        f"<html><body style='{base_css}'><h1>{WAL_TITLE}</h1><p>{WAL_BODY}</p>"
        "<p>Checkpoint progress decides which frames stay visible.</p></body></html>",
        [{"page": 1, "text": "WAL readers continue from a stable end mark"}])
    pdf("two-column.pdf",
        f"<html><body style='{base_css}'><h1>{WAL_TITLE}</h1>"
        "<div style='columns:2;column-gap:36px'><p>Left column pins the reader end mark. "
        "Right column appends writer frames after the mark.</p>"
        "<p>Both columns describe checkpoint progress and retention windows.</p></div></body></html>",
        [{"page": 1, "text": "Left column pins the reader end mark"},
         {"page": 1, "text": "Right column appends writer frames"}])
    pdf("equations.pdf",
        f"<html><body style='{base_css}'><h1>Retention bounds</h1>"
        "<p>The visible set is base plus frames f where f &#8804; end: "
        "V = B &#8746; {f &#8712; F : f &#8804; e}.</p>"
        "<p>Checkpoint advance is monotone: e(t+1) &#8805; e(t).</p></body></html>",
        [{"page": 1, "text": "Checkpoint advance is monotone"},
         {"page": 1, "text": "e(t+1) ≥ e(t)"}])
    pdf("diagrams.pdf",
        f"<html><body style='{base_css}'><h1>Frame flow</h1>"
        "<svg width='400' height='120' xmlns='http://www.w3.org/2000/svg'>"
        "<rect x='10' y='30' width='110' height='50' fill='none' stroke='black'/>"
        "<text x='20' y='60'>base read</text>"
        "<line x1='120' y1='55' x2='170' y2='55' stroke='black'/>"
        "<rect x='170' y='30' width='110' height='50' fill='none' stroke='black'/>"
        "<text x='180' y='60'>frame filter</text></svg>"
        "<p>Caption: the base read stays unconditional while frames filter by the end mark.</p></body></html>",
        [{"page": 1, "text": "the base read stays unconditional"}])
    pdf("tables.pdf",
        f"<html><body style='{base_css}'><h1>Checkpoint table</h1>"
        "<table border='1'><tr><th>mark</th><th>visible</th></tr>"
        "<tr><td>end</td><td>pinned base</td></tr>"
        "<tr><td>advanced</td><td>base plus frames</td></tr></table>"
        "<p>Second table lists retention windows per checkpoint epoch.</p>"
        "<table border='1'><tr><th>epoch</th><th>window</th></tr>"
        "<tr><td>41</td><td>stable</td></tr></table></body></html>",
        [{"page": 1, "text": "pinned base"}, {"page": 1, "text": "retention windows"}])

    scanned_words = scanned_pdf_from_text(
        chrome,
        ["SCANNED WAL NOTE", "READERS PIN THE END MARK.",
         "LATER COMMITS STAY INVISIBLE."],
        os.path.join(out, "scanned.pdf"),
    )
    manifest.append({"file": "scanned.pdf", "kind": "pdf",
                     "anchors": [], "ocr_text": scanned_words,
                     "native": {"support": "text_unavailable", "coverage": "text_unavailable",
                                "anchor": {"page": 1}, "query": "READERS PIN THE END MARK"},
                     "note": "no text layer; native extraction must report text_unavailable"})

    docx_a = minimal_docx(
        [WAL_TITLE, WAL_BODY, "Retention windows depend on checkpoint progress."],
        tables=[[["mark", "visible"], ["end", "pinned base"]]],
    )
    write_zip(os.path.join(out, "notes.docx"), docx_a)
    manifest.append({"file": "notes.docx", "kind": "docx",
                     "anchors": [{"section": "body", "text": "WAL readers continue from a stable end mark"},
                                 {"section": "table", "text": "pinned base"}]})
    docx_b = minimal_docx(
        ["Checkpoint review", "Spot-check old high-dependency knowledge.",
         "Revisit the end-mark rule after contradiction."],
    )
    write_zip(os.path.join(out, "review.docx"), docx_b)
    manifest.append({"file": "review.docx", "kind": "docx",
                     "anchors": [{"section": "body", "text": "Spot-check old high-dependency knowledge"}]})

    write_zip(os.path.join(out, "lesson.pptx"), minimal_pptx([
        [WAL_TITLE, WAL_BODY],
        ["Checkpoint progress", "Retention windows depend on checkpoint progress."],
    ]))
    manifest.append({"file": "lesson.pptx", "kind": "pptx",
                     "anchors": [{"slide": 1, "text": WAL_TITLE},
                                 {"slide": 2, "text": "Retention windows depend on checkpoint progress"}]})
    write_zip(os.path.join(out, "recap.pptx"), minimal_pptx([
        ["End-mark recap", "The base read stays unconditional."],
    ]))
    manifest.append({"file": "recap.pptx", "kind": "pptx",
                     "anchors": [{"slide": 1, "text": "The base read stays unconditional"}]})

    for name, title, chapters in [
        ("course.epub", WAL_TITLE, [
            ("End marks", WAL_BODY),
            ("Checkpoints", "Retention windows depend on checkpoint progress."),
        ]),
        ("glossary.epub", "WAL glossary", [
            ("Frame", "An appended writer unit after the end mark."),
            ("Checkpoint", "Progress that decides which frames stay visible."),
        ]),
    ]:
        parts = minimal_epub(title, chapters)
        mimetype = ("mimetype", "application/epub+zip")
        write_zip(os.path.join(out, name), [p for p in parts],
                  stored_first=mimetype)
        manifest.append({"file": name, "kind": "epub",
                         "anchors": [{"chapter": i + 1, "text": body[:60]}
                                     for i, (_, body) in enumerate(chapters)]})

    for entry in manifest:
        if entry["kind"] != "pdf":
            entry["native"] = {"support": "unsupported", "coverage": "unsupported"}

    with open(os.path.join(out, "manifest.json"), "w") as handle:
        import json

        json.dump({"documents": manifest}, handle, indent=2)
    print(f"sample generated: {len(manifest)} documents in {out}")


if __name__ == "__main__":
    main()
