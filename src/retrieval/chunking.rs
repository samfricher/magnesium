use std::path::Path;

use serde::{Deserialize, Serialize};

const MAX_CHUNK_BYTES: usize = 16 * 1024;
const MAX_CHUNK_LINES: usize = 120;
const OVERLAP_LINES: usize = 8;
const MAX_METADATA_TEXT_BYTES: usize = 256;
const MAX_STRUCTURAL_PATH_BYTES: usize = 1_024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChunkKind {
    Fallback,
    Heading,
    Page,
    Module,
    Symbol,
}

impl ChunkKind {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Fallback => "fallback",
            Self::Heading => "heading",
            Self::Page => "page",
            Self::Module => "module",
            Self::Symbol => "symbol",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "fallback" => Some(Self::Fallback),
            "heading" => Some(Self::Heading),
            "page" => Some(Self::Page),
            "module" => Some(Self::Module),
            "symbol" => Some(Self::Symbol),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SymbolDefinition {
    pub(crate) name: String,
    pub(crate) kind: String,
    pub(crate) line_start: u32,
    pub(crate) line_end: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SourceImport {
    pub(crate) module: String,
    pub(crate) names: Vec<String>,
    pub(crate) line_start: u32,
    pub(crate) line_end: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SymbolReference {
    pub(crate) name: String,
    pub(crate) line: u32,
}

#[derive(Clone, Debug)]
pub(super) struct ChunkDraft {
    pub(super) kind: ChunkKind,
    pub(super) line_start: u32,
    pub(super) line_end: u32,
    pub(super) body: String,
    pub(super) structural_path: Option<String>,
    pub(super) language: Option<&'static str>,
    pub(super) definitions: Vec<SymbolDefinition>,
    pub(super) imports: Vec<SourceImport>,
    pub(super) references: Vec<SymbolReference>,
}

pub(super) fn chunk_text(path: &Path, text: &str) -> Vec<ChunkDraft> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("md" | "markdown" | "mdx" | "qmd" | "rmd") => {
            chunk_sections(text, &markdown_headings(text))
        }
        Some("html" | "htm") => chunk_sections(text, &html_headings(text)),
        Some("tex") => chunk_sections(text, &latex_headings(text)),
        Some("ipynb") => chunk_notebook(text)
            .unwrap_or_else(|| bounded_chunks(text, 1, ChunkKind::Fallback, None)),
        _ => bounded_chunks(text, 1, ChunkKind::Fallback, None),
    }
}

pub(super) fn plain_text(path: &Path, text: &str) -> Option<String> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("html" | "htm") => Some(decode_entities(&strip_tags_by_line(text))),
        Some("ipynb") => {
            let notebook: serde_json::Value = serde_json::from_str(text).ok()?;
            let mut strings = Vec::new();
            json_strings(&notebook, &mut strings);
            Some(strings.join("\n"))
        }
        _ => None,
    }
}

fn strip_tags_by_line(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '\n' => text.push('\n'),
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    text
}

fn json_strings(value: &serde_json::Value, strings: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => strings.push(text.clone()),
        serde_json::Value::Array(items) if items.iter().all(serde_json::Value::is_string) => {
            strings.push(items.iter().filter_map(serde_json::Value::as_str).collect());
        }
        serde_json::Value::Array(items) => {
            for item in items {
                json_strings(item, strings);
            }
        }
        serde_json::Value::Object(fields) => {
            for item in fields.values() {
                json_strings(item, strings);
            }
        }
        _ => {}
    }
}

pub(super) fn page_chunk(page: u32, body: String, title: Option<String>) -> ChunkDraft {
    ChunkDraft {
        kind: ChunkKind::Page,
        line_start: page,
        line_end: page,
        body,
        structural_path: Some(
            title
                .map(|title| bounded_text(&title, MAX_STRUCTURAL_PATH_BYTES))
                .unwrap_or_else(|| format!("page {page}")),
        ),
        language: None,
        definitions: Vec::new(),
        imports: Vec::new(),
        references: Vec::new(),
    }
}

type Heading = Option<(usize, String)>;

fn chunk_sections(text: &str, headings: &[Heading]) -> Vec<ChunkDraft> {
    let lines = line_ranges(text);
    if lines.is_empty() {
        return bounded_chunks(text, 1, ChunkKind::Fallback, None);
    }
    let mut stack = Vec::new();
    let mut chunks = Vec::new();
    emit_sections(text, &lines, headings, 0, &mut stack, &mut chunks);
    chunks
}

fn emit_sections(
    text: &str,
    lines: &[LineRange],
    headings: &[Heading],
    line_offset: u32,
    stack: &mut Vec<(usize, String)>,
    chunks: &mut Vec<ChunkDraft>,
) {
    let mut sections = Vec::new();
    let mut start = 0;
    let mut current_path = heading_path(stack);
    let mut opens = false;
    for (index, heading) in headings.iter().enumerate().take(lines.len()) {
        let Some((level, title)) = heading else {
            continue;
        };
        if index > start {
            sections.push((start, index, current_path.clone(), opens));
        }
        opens = true;
        while stack.last().is_some_and(|(top, _)| *top >= *level) {
            stack.pop();
        }
        stack.push((*level, bounded_text(title, MAX_METADATA_TEXT_BYTES)));
        current_path = heading_path(stack);
        start = index;
    }
    if start < lines.len() {
        sections.push((start, lines.len(), current_path, opens));
    }
    for (start, end, path, opens) in sections {
        let body = &text[lines[start].start..lines[end - 1].end];
        let kind = if path.is_some() && opens {
            ChunkKind::Heading
        } else {
            ChunkKind::Fallback
        };
        let first = chunks.len();
        chunks.extend(bounded_chunks(
            body,
            lines[start].number.saturating_add(line_offset),
            kind,
            path,
        ));
        for continuation in chunks.iter_mut().skip(first + 1) {
            continuation.kind = ChunkKind::Fallback;
        }
    }
}

fn heading_path(stack: &[(usize, String)]) -> Option<String> {
    (!stack.is_empty()).then(|| {
        bounded_text(
            &stack
                .iter()
                .map(|(_, text)| text.as_str())
                .collect::<Vec<_>>()
                .join(" > "),
            MAX_STRUCTURAL_PATH_BYTES,
        )
    })
}

fn markdown_headings(text: &str) -> Vec<Heading> {
    let mut fence = None;
    line_ranges(text)
        .iter()
        .map(|line| {
            let content = text[line.start..line.end].trim_end_matches(['\r', '\n']);
            let trimmed = content.trim_start();
            if let Some((marker, width)) = fence {
                if trimmed.bytes().take_while(|byte| *byte == marker).count() >= width {
                    fence = None;
                }
                return None;
            }
            let marker = trimmed.as_bytes().first().copied();
            let width = marker
                .filter(|marker| matches!(marker, b'`' | b'~'))
                .map(|marker| trimmed.bytes().take_while(|byte| *byte == marker).count())
                .unwrap_or(0);
            if width >= 3 {
                fence = marker.map(|marker| (marker, width));
                return None;
            }
            markdown_heading(content)
        })
        .collect()
}

fn markdown_heading(line: &str) -> Heading {
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level) || line.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    let mut heading = line[level + 1..].trim();
    if heading.ends_with('}')
        && let Some(open) = heading.rfind(" {")
    {
        heading = heading[..open].trim_end();
    }
    let unclosed = heading.trim_end_matches('#');
    if unclosed.len() < heading.len() && (unclosed.is_empty() || unclosed.ends_with(' ')) {
        heading = unclosed.trim_end();
    }
    (!heading.is_empty()).then(|| (level, heading.to_owned()))
}

fn html_headings(text: &str) -> Vec<Heading> {
    let lines = line_ranges(text);
    let mut headings = vec![None; lines.len()];
    let lower = text.to_ascii_lowercase();
    let mut at = 0;
    while let Some(found) = lower[at..].find('<') {
        let open = at + found;
        let rest = &lower[open..];
        if let Some(skip) = ["<script", "<style", "<!--"]
            .iter()
            .find(|tag| rest.starts_with(**tag))
        {
            let close = match *skip {
                "<script" => "</script",
                "<style" => "</style",
                _ => "-->",
            };
            at = lower[open..]
                .find(close)
                .map_or(lower.len(), |end| open + end + close.len());
            continue;
        }
        let level = rest
            .as_bytes()
            .get(1)
            .filter(|byte| **byte == b'h')
            .and(rest.as_bytes().get(2))
            .filter(|digit| (b'1'..=b'6').contains(*digit))
            .filter(|_| {
                rest.as_bytes()
                    .get(3)
                    .is_some_and(|next| *next == b'>' || next.is_ascii_whitespace())
            })
            .map(|digit| usize::from(digit - b'0'));
        let Some(level) = level else {
            at = open + 1;
            continue;
        };
        let Some(body_start) = lower[open..].find('>').map(|end| open + end + 1) else {
            break;
        };
        let close = format!("</h{level}");
        let Some(body_end) = lower[body_start..].find(&close).map(|end| body_start + end) else {
            at = body_start;
            continue;
        };
        let inner = &lower[body_start..body_end];
        let cut = [
            "<div", "<p", "<ul", "<ol", "<button", "<table", "<form", "<nav",
        ]
        .iter()
        .filter_map(|tag| inner.find(tag))
        .min()
        .map_or(body_end, |offset| body_start + offset);
        let title = html_text(&text[body_start..cut]);
        let line = lines.partition_point(|line| line.end <= open);
        if !title.is_empty() && line < headings.len() && headings[line].is_none() {
            headings[line] = Some((level, title));
        }
        at = body_end;
    }
    headings
}

fn html_text(fragment: &str) -> String {
    let mut text = String::new();
    let mut in_tag = false;
    for character in fragment.chars() {
        match character {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let decoded = decode_entities(&text);
    decoded
        .replace(['¶', '#'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.bytes().take(12).position(|byte| byte == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(character) => {
                out.push(character);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn latex_headings(text: &str) -> Vec<Heading> {
    const COMMANDS: &[(&str, usize)] = &[
        ("\\chapter", 1),
        ("\\section", 2),
        ("\\subsection", 3),
        ("\\subsubsection", 4),
        ("\\begin{frame}", 5),
        ("\\frametitle", 5),
    ];
    line_ranges(text)
        .iter()
        .map(|line| {
            let content = text[line.start..line.end].trim_end_matches(['\r', '\n']);
            let code = content.split('%').next().unwrap_or("");
            COMMANDS.iter().find_map(|(command, level)| {
                let at = code.find(command)?;
                let after = &code[at + command.len()..];
                if after
                    .chars()
                    .next()
                    .is_some_and(|next| next.is_ascii_alphabetic())
                {
                    return None;
                }
                let after = after.strip_prefix('*').unwrap_or(after).trim_start();
                let after = match after.strip_prefix('[') {
                    Some(options) => &options[options.find(']')? + 1..],
                    None => after,
                }
                .trim_start();
                let title = latex_text(braced(after)?);
                (!title.is_empty()).then_some((*level, title))
            })
        })
        .collect()
}

fn braced(text: &str) -> Option<&str> {
    let inner = text.strip_prefix('{')?;
    let mut depth = 1_usize;
    for (index, character) in inner.char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&inner[..index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn latex_text(source: &str) -> String {
    let mut text = String::new();
    let mut chars = source.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\\' => {
                let mut name = String::new();
                while let Some(next) = chars.peek().copied().filter(char::is_ascii_alphabetic) {
                    name.push(next);
                    chars.next();
                }
                if name.is_empty() {
                    if let Some(next) = chars.next()
                        && next != '\\'
                    {
                        text.push(next);
                    }
                    text.push(' ');
                } else if name == "label" {
                    let mut depth = 0_usize;
                    for next in chars.by_ref() {
                        match next {
                            '{' => depth += 1,
                            '}' => {
                                depth = depth.saturating_sub(1);
                                if depth == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    text.push(' ');
                }
            }
            '{' | '}' => {}
            '~' => text.push(' '),
            _ => text.push(character),
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn chunk_notebook(text: &str) -> Option<Vec<ChunkDraft>> {
    let notebook: serde_json::Value = serde_json::from_str(text).ok()?;
    let cells = notebook.get("cells")?.as_array()?;
    let raw = line_ranges(text);
    let mut cursor = 0;
    let mut stack = Vec::new();
    let mut chunks = Vec::new();
    for cell in cells {
        let pieces = match cell.get("source")? {
            serde_json::Value::Array(pieces) => pieces
                .iter()
                .map(|piece| piece.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()?,
            serde_json::Value::String(whole) if !whole.contains('\n') => vec![whole.clone()],
            serde_json::Value::String(whole) if whole.is_empty() => Vec::new(),
            _ => return None,
        };
        if pieces.is_empty() {
            continue;
        }
        let first = serde_json::to_string(&pieces[0]).ok()?;
        let found = (cursor..raw.len()).find(|&index| {
            text[raw[index].start..raw[index].end]
                .trim_start()
                .starts_with(&first)
        })?;
        for (offset, piece) in pieces.iter().enumerate().skip(1) {
            let encoded = serde_json::to_string(piece).ok()?;
            let line = raw.get(found + offset)?;
            if !text[line.start..line.end]
                .trim_start()
                .starts_with(&encoded)
            {
                return None;
            }
            if !pieces[offset - 1].ends_with('\n') {
                return None;
            }
        }
        cursor = found + pieces.len();
        let mut body = pieces.concat();
        if !body.ends_with('\n') {
            body.push('\n');
        }
        let lines = line_ranges(&body);
        let headings =
            if cell.get("cell_type").and_then(serde_json::Value::as_str) == Some("markdown") {
                markdown_headings(&body)
            } else {
                vec![None; lines.len()]
            };
        let line_offset = raw[found].number - 1;
        emit_sections(
            &body,
            &lines,
            &headings,
            line_offset,
            &mut stack,
            &mut chunks,
        );
    }
    (!chunks.is_empty()).then_some(chunks)
}

fn bounded_chunks(
    text: &str,
    base_line: u32,
    kind: ChunkKind,
    structural_path: Option<String>,
) -> Vec<ChunkDraft> {
    let lines = line_ranges(text);
    if lines.is_empty() {
        return vec![empty_chunk(base_line, kind, structural_path)];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let first = &lines[start];
        if first.end - first.start > MAX_CHUNK_BYTES {
            let mut byte = first.start;
            while byte < first.end {
                let end = utf8_boundary_at_or_before(text, (byte + MAX_CHUNK_BYTES).min(first.end));
                let end =
                    end.max(byte + text[byte..].chars().next().map(char::len_utf8).unwrap_or(1));
                chunks.push(draft(
                    kind,
                    base_line.saturating_add(first.number - 1),
                    base_line.saturating_add(first.number - 1),
                    text[byte..end].to_owned(),
                    structural_path.clone(),
                ));
                byte = end;
            }
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < lines.len()
            && end - start < MAX_CHUNK_LINES
            && lines[end].end - first.start <= MAX_CHUNK_BYTES
        {
            end += 1;
        }
        chunks.push(draft(
            kind,
            base_line.saturating_add(first.number - 1),
            base_line.saturating_add(lines[end - 1].number - 1),
            text[first.start..lines[end - 1].end].to_owned(),
            structural_path.clone(),
        ));
        if end == lines.len() {
            break;
        }
        let overlap = if kind == ChunkKind::Fallback && end - start > 2 * OVERLAP_LINES {
            OVERLAP_LINES
        } else {
            0
        };
        start = end.saturating_sub(overlap).max(start + 1);
    }
    chunks
}

fn empty_chunk(line: u32, kind: ChunkKind, structural_path: Option<String>) -> ChunkDraft {
    draft(kind, line, line, String::new(), structural_path)
}

fn draft(
    kind: ChunkKind,
    line_start: u32,
    line_end: u32,
    body: String,
    structural_path: Option<String>,
) -> ChunkDraft {
    ChunkDraft {
        kind,
        line_start,
        line_end,
        body,
        structural_path,
        language: None,
        definitions: Vec::new(),
        imports: Vec::new(),
        references: Vec::new(),
    }
}

#[derive(Clone, Copy)]
struct LineRange {
    start: usize,
    end: usize,
    number: u32,
}

fn line_ranges(text: &str) -> Vec<LineRange> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut number = 1_u32;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            lines.push(LineRange {
                start,
                end: index + 1,
                number,
            });
            start = index + 1;
            number = number.saturating_add(1);
        }
    }
    if start < text.len() {
        lines.push(LineRange {
            start,
            end: text.len(),
            number,
        });
    }
    lines
}

fn utf8_boundary_at_or_before(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    let end = utf8_boundary_at_or_before(value, value.len().min(max_bytes));
    value[..end].to_owned()
}

#[cfg(test)]
mod tests;
