// A small PDF made on the page, for the demo and for the window's own check.
// Nothing here is a real course: each page holds one made-up line.

/** A PDF with one page for each line. Standard fonts only, ASCII text only. */
export function tinyPdf(lines: string[]): Uint8Array {
  const escape = (text: string) => text.replace(/[\\()]/g, (c) => `\\${c}`);
  const pages = lines.length;
  // 1 catalog, 2 pages, 3 font, then a page and its content for each line.
  const objects: string[] = [
    "<< /Type /Catalog /Pages 2 0 R >>",
    `<< /Type /Pages /Kids [${lines.map((_, n) => `${4 + n * 2} 0 R`).join(" ")}] /Count ${pages} >>`,
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
  ];
  lines.forEach((line, n) => {
    const content = [
      "BT /F1 30 Tf 60 700 Td (Week 4 slides) Tj ET",
      `BT /F1 22 Tf 60 640 Td (${escape(line)}) Tj ET`,
      `BT /F1 14 Tf 60 60 Td (p. ${n + 1}) Tj ET`,
      "0.2 0.5 0.4 rg 60 560 300 6 re f",
    ].join("\n");
    objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents ${5 + n * 2} 0 R /Resources << /Font << /F1 3 0 R >> >> >>`);
    objects.push(`<< /Length ${content.length} >>\nstream\n${content}\nendstream`);
  });
  let body = "%PDF-1.4\n";
  const offsets: number[] = [];
  objects.forEach((object, index) => {
    offsets.push(body.length);
    body += `${index + 1} 0 obj\n${object}\nendobj\n`;
  });
  const xref = body.length;
  body += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  for (const offset of offsets) body += `${String(offset).padStart(10, "0")} 00000 n \n`;
  body += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return new TextEncoder().encode(body);
}
