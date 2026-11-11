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
    /// `Module` and `Symbol` pieces were cut from Python, JavaScript and
    /// TypeScript files by earlier versions. New code files are split as
    /// plain text; an index built earlier still holds and reads these.
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

/// Code metadata that earlier versions stored with each code piece. New
/// pieces carry none, but search still reads it from an earlier index.
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
        Some("md" | "markdown" | "mdx") => chunk_markdown(text),
        _ => bounded_chunks(text, 1, ChunkKind::Fallback, None),
    }
}

pub(super) fn page_chunk(page: u32, body: String) -> ChunkDraft {
    ChunkDraft {
        kind: ChunkKind::Page,
        line_start: page,
        line_end: page,
        body,
        structural_path: Some(format!("page {page}")),
        language: None,
        definitions: Vec::new(),
        imports: Vec::new(),
        references: Vec::new(),
    }
}

fn chunk_markdown(text: &str) -> Vec<ChunkDraft> {
    let lines = line_ranges(text);
    if lines.is_empty() {
        return bounded_chunks(text, 1, ChunkKind::Fallback, None);
    }
    let mut sections = Vec::new();
    let mut start = 0;
    let mut current_path = None;
    let mut heading_stack = Vec::<(usize, String)>::new();
    let mut fence = None;
    for (index, line) in lines.iter().enumerate() {
        let content = text[line.start..line.end].trim_end_matches(['\r', '\n']);
        let trimmed = content.trim_start();
        if let Some((marker, width)) = fence {
            if trimmed.bytes().take_while(|byte| *byte == marker).count() >= width {
                fence = None;
            }
            continue;
        }
        let marker = trimmed.as_bytes().first().copied();
        let width = marker
            .filter(|marker| matches!(marker, b'`' | b'~'))
            .map(|marker| trimmed.bytes().take_while(|byte| *byte == marker).count())
            .unwrap_or(0);
        if width >= 3 {
            fence = marker.map(|marker| (marker, width));
            continue;
        }
        let Some((level, heading)) = markdown_heading(content) else {
            continue;
        };
        if index > start {
            sections.push((start, index, current_path.clone()));
        }
        while heading_stack.last().is_some_and(|(top, _)| *top >= level) {
            heading_stack.pop();
        }
        heading_stack.push((level, bounded_text(heading, MAX_METADATA_TEXT_BYTES)));
        current_path = Some(bounded_text(
            &heading_stack
                .iter()
                .map(|(_, text)| text.as_str())
                .collect::<Vec<_>>()
                .join(" > "),
            MAX_STRUCTURAL_PATH_BYTES,
        ));
        start = index;
    }
    if start < lines.len() {
        sections.push((start, lines.len(), current_path));
    }
    let mut chunks = Vec::new();
    for (start, end, path) in sections {
        let body = &text[lines[start].start..lines[end - 1].end];
        let kind = if path.is_some() {
            ChunkKind::Heading
        } else {
            ChunkKind::Fallback
        };
        chunks.extend(bounded_chunks(body, lines[start].number, kind, path));
    }
    chunks
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|byte| *byte == b'#').count();
    if !(1..=6).contains(&level) || line.as_bytes().get(level) != Some(&b' ') {
        return None;
    }
    let heading = line[level + 1..].trim();
    (!heading.is_empty()).then_some((level, heading))
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
        // Overlap only chunks long enough that it stays under half of each one.
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
