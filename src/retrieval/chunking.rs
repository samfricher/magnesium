use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Language, Node, Parser, Point};

const MAX_CHUNK_BYTES: usize = 16 * 1024;
const MAX_CHUNK_LINES: usize = 120;
const OVERLAP_LINES: usize = 8;
const MAX_IMPORTS_PER_CHUNK: usize = 128;
const MAX_DEFINITIONS_PER_CHUNK: usize = 256;
const MAX_REFERENCES_PER_CHUNK: usize = 512;
const MAX_IMPORT_NAMES: usize = 64;
const MAX_METADATA_TEXT_BYTES: usize = 256;
const MAX_IMPORT_TEXT_BYTES_PER_CHUNK: usize = 8 * 1_024;
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
        Some("md" | "markdown" | "mdx") => chunk_markdown(text),
        Some("py") => chunk_code(text, tree_sitter_python::LANGUAGE.into(), "python"),
        Some("js" | "mjs" | "cjs" | "jsx") => {
            chunk_code(text, tree_sitter_javascript::LANGUAGE.into(), "javascript")
        }
        Some("ts" | "mts" | "cts") => chunk_code(
            text,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript",
        ),
        Some("tsx") => chunk_code(text, tree_sitter_typescript::LANGUAGE_TSX.into(), "tsx"),
        _ => bounded_chunks(text, 1, ChunkKind::Fallback, None, None),
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
        return bounded_chunks(text, 1, ChunkKind::Fallback, None, None);
    }
    let mut sections = Vec::new();
    let mut start = 0;
    let mut current_path = None;
    let mut heading_stack = Vec::<String>::new();
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
        heading_stack.truncate(level.saturating_sub(1));
        heading_stack.push(bounded_text(heading, MAX_METADATA_TEXT_BYTES));
        current_path = Some(bounded_text(
            &heading_stack.join(" > "),
            MAX_STRUCTURAL_PATH_BYTES,
        ));
        start = index;
    }
    if start < lines.len() {
        sections.push((start, lines.len(), current_path));
    }
    if sections.is_empty() {
        return bounded_chunks(text, 1, ChunkKind::Fallback, None, None);
    }
    let mut chunks = Vec::new();
    for (start, end, path) in sections {
        let body = &text[lines[start].start..lines[end - 1].end];
        let kind = if path.is_some() {
            ChunkKind::Heading
        } else {
            ChunkKind::Fallback
        };
        chunks.extend(bounded_chunks(body, lines[start].number, kind, path, None));
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

fn chunk_code(text: &str, language: Language, language_name: &'static str) -> Vec<ChunkDraft> {
    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return bounded_chunks(text, 1, ChunkKind::Fallback, None, Some(language_name));
    }
    let Some(tree) = parser.parse(text, None) else {
        return bounded_chunks(text, 1, ChunkKind::Fallback, None, Some(language_name));
    };
    let root = tree.root_node();
    let file_imports = imports_for(root, text);
    let module_references = references_for(root, text, &[]);
    let symbols = top_level_symbols(root);
    if symbols.is_empty() {
        let mut chunks = bounded_chunks(text, 1, ChunkKind::Module, None, Some(language_name));
        annotate_module_chunks(&mut chunks, &file_imports, &module_references);
        return chunks;
    }
    let mut chunks = Vec::new();
    let mut previous_byte = 0;
    for symbol in symbols {
        if symbol.start_byte() > previous_byte {
            let gap = &text[previous_byte..symbol.start_byte()];
            let start_line = line_at_byte(text, previous_byte);
            if !gap.trim().is_empty() {
                let mut gap_chunks = bounded_chunks(
                    gap,
                    start_line,
                    ChunkKind::Module,
                    None,
                    Some(language_name),
                );
                annotate_module_chunks(&mut gap_chunks, &file_imports, &module_references);
                chunks.extend(gap_chunks);
            }
        }
        chunks.extend(symbol_chunks(symbol, text, language_name));
        previous_byte = symbol.end_byte();
    }
    if previous_byte < text.len() {
        let gap = &text[previous_byte..];
        if !gap.trim().is_empty() {
            let mut gap_chunks = bounded_chunks(
                gap,
                line_at_byte(text, previous_byte),
                ChunkKind::Module,
                None,
                Some(language_name),
            );
            annotate_module_chunks(&mut gap_chunks, &file_imports, &module_references);
            chunks.extend(gap_chunks);
        }
    }
    chunks
}

fn annotate_module_chunks(
    chunks: &mut [ChunkDraft],
    imports: &[SourceImport],
    references: &[SymbolReference],
) {
    for chunk in chunks {
        chunk.imports = imports_in_lines(imports, chunk.line_start, chunk.line_end);
        chunk.references = references
            .iter()
            .filter(|reference| (chunk.line_start..=chunk.line_end).contains(&reference.line))
            .cloned()
            .collect();
    }
}

fn top_level_symbols(root: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = root.walk();
    root.named_children(&mut cursor)
        .filter_map(|node| {
            if is_symbol_node(node.kind()) {
                Some(node)
            } else if matches!(node.kind(), "decorated_definition" | "export_statement") {
                let mut nested = node.walk();
                node.named_children(&mut nested)
                    .find(|child| is_symbol_node(child.kind()))
                    .map(|_| node)
            } else {
                None
            }
        })
        .collect()
}

fn symbol_chunks(node: Node<'_>, source: &str, language: &'static str) -> Vec<ChunkDraft> {
    let body = &source[node.start_byte()..node.end_byte()];
    let definitions = definitions_for(node, source);
    let imports = imports_for(node, source);
    let structural_path = definitions
        .first()
        .map(|definition| definition.name.clone());
    let references = references_for(node, source, &definitions);
    let mut chunks = bounded_chunks(
        body,
        point_line(node.start_position()),
        ChunkKind::Symbol,
        structural_path,
        Some(language),
    );
    for chunk in &mut chunks {
        chunk.definitions =
            metadata_in_lines(&definitions, chunk.line_start, chunk.line_end, |value| {
                (value.line_start, value.line_end)
            });
        chunk.imports = imports_in_lines(&imports, chunk.line_start, chunk.line_end);
        chunk.references = references
            .iter()
            .filter(|reference| (chunk.line_start..=chunk.line_end).contains(&reference.line))
            .cloned()
            .collect();
    }
    chunks
}

fn metadata_in_lines<T: Clone>(
    values: &[T],
    start: u32,
    end: u32,
    lines: impl Fn(&T) -> (u32, u32),
) -> Vec<T> {
    values
        .iter()
        .filter(|value| {
            let (value_start, value_end) = lines(value);
            value_start <= end && value_end >= start
        })
        .cloned()
        .collect()
}

fn imports_in_lines(values: &[SourceImport], start: u32, end: u32) -> Vec<SourceImport> {
    let mut remaining = MAX_IMPORT_TEXT_BYTES_PER_CHUNK;
    let mut imports = Vec::new();
    for value in values
        .iter()
        .filter(|value| value.line_start <= end && value.line_end >= start)
    {
        if value.module.len() > remaining {
            break;
        }
        remaining -= value.module.len();
        let mut value = value.clone();
        value.names.retain(|name| {
            if name.len() > remaining {
                return false;
            }
            remaining -= name.len();
            true
        });
        imports.push(value);
    }
    imports
}

fn definitions_for(node: Node<'_>, source: &str) -> Vec<SymbolDefinition> {
    let mut definitions = Vec::new();
    walk(node, &mut |candidate| {
        let Some(kind) = definition_kind(candidate.kind()) else {
            return;
        };
        let name_node = candidate.child_by_field_name("name").or_else(|| {
            (candidate.kind() == "variable_declarator")
                .then(|| candidate.named_child(0))
                .flatten()
        });
        let Some(name_node) = name_node else {
            return;
        };
        let Ok(name) = name_node.utf8_text(source.as_bytes()) else {
            return;
        };
        if !name.trim().is_empty() && definitions.len() < MAX_DEFINITIONS_PER_CHUNK {
            definitions.push(SymbolDefinition {
                name: bounded_text(name, MAX_METADATA_TEXT_BYTES),
                kind: kind.to_owned(),
                line_start: point_line(candidate.start_position()),
                line_end: end_line(candidate),
            });
        }
    });
    definitions
}

fn imports_for(node: Node<'_>, source: &str) -> Vec<SourceImport> {
    let mut imports = Vec::new();
    walk(node, &mut |candidate| {
        if imports.len() >= MAX_IMPORTS_PER_CHUNK {
            return;
        }
        let is_import = matches!(
            candidate.kind(),
            "import_statement" | "import_from_statement" | "import_declaration"
        );
        let is_require = candidate.kind() == "call_expression"
            && candidate
                .child_by_field_name("function")
                .and_then(|function| function.utf8_text(source.as_bytes()).ok())
                == Some("require");
        if !is_import && !is_require {
            return;
        }
        let Ok(statement) = candidate.utf8_text(source.as_bytes()) else {
            return;
        };
        let module = import_module(candidate, statement, source);
        let names = identifier_names(candidate, source, MAX_IMPORT_NAMES);
        imports.push(SourceImport {
            module,
            names,
            line_start: point_line(candidate.start_position()),
            line_end: end_line(candidate),
        });
    });
    imports
}

fn import_module(node: Node<'_>, statement: &str, source: &str) -> String {
    let mut module = None;
    walk(node, &mut |candidate| {
        if module.is_none() && candidate.kind() == "string" {
            module = candidate.utf8_text(source.as_bytes()).ok().map(|value| {
                bounded_text(value.trim_matches(['\'', '"']), MAX_METADATA_TEXT_BYTES)
            });
        }
    });
    module.unwrap_or_else(|| {
        let trimmed = statement.trim();
        if let Some(rest) = trimmed.strip_prefix("from ") {
            bounded_text(
                rest.split_whitespace().next().unwrap_or(trimmed),
                MAX_METADATA_TEXT_BYTES,
            )
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            bounded_text(
                rest.split([',', ' ', ';']).next().unwrap_or(trimmed),
                MAX_METADATA_TEXT_BYTES,
            )
        } else {
            bounded_text(trimmed, MAX_METADATA_TEXT_BYTES)
        }
    })
}

fn references_for(
    node: Node<'_>,
    source: &str,
    definitions: &[SymbolDefinition],
) -> Vec<SymbolReference> {
    let definition_names = definitions
        .iter()
        .map(|definition| definition.name.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut references = Vec::new();
    walk(node, &mut |candidate| {
        if references.len() >= MAX_REFERENCES_PER_CHUNK
            || !matches!(
                candidate.kind(),
                "identifier"
                    | "property_identifier"
                    | "type_identifier"
                    | "shorthand_property_identifier"
            )
        {
            return;
        }
        let Ok(name) = candidate.utf8_text(source.as_bytes()) else {
            return;
        };
        let line = point_line(candidate.start_position());
        let name = bounded_text(name, MAX_METADATA_TEXT_BYTES);
        if !definition_names.contains(name.as_str()) && seen.insert((name.clone(), line)) {
            references.push(SymbolReference { name, line });
        }
    });
    references
}

fn identifier_names(node: Node<'_>, source: &str, limit: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    walk(node, &mut |candidate| {
        if names.len() < limit
            && matches!(candidate.kind(), "identifier" | "type_identifier")
            && let Ok(name) = candidate.utf8_text(source.as_bytes())
        {
            let name = bounded_text(name, MAX_METADATA_TEXT_BYTES);
            if seen.insert(name.clone()) {
                names.push(name);
            }
        }
    });
    names
}

fn definition_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "function_definition" | "function_declaration" | "generator_function_declaration" => {
            Some("function")
        }
        "class_definition" | "class_declaration" => Some("class"),
        "method_definition" => Some("method"),
        "interface_declaration" => Some("interface"),
        "type_alias_declaration" => Some("type_alias"),
        "enum_declaration" => Some("enum"),
        "variable_declarator" => Some("variable"),
        _ => None,
    }
}

fn is_symbol_node(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition"
            | "function_declaration"
            | "generator_function_declaration"
            | "class_definition"
            | "class_declaration"
            | "interface_declaration"
            | "type_alias_declaration"
            | "enum_declaration"
            | "lexical_declaration"
            | "variable_declaration"
    )
}

fn walk(node: Node<'_>, visit: &mut impl FnMut(Node<'_>)) {
    visit(node);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, visit);
    }
}

fn bounded_chunks(
    text: &str,
    base_line: u32,
    kind: ChunkKind,
    structural_path: Option<String>,
    language: Option<&'static str>,
) -> Vec<ChunkDraft> {
    let lines = line_ranges(text);
    if lines.is_empty() {
        return vec![empty_chunk(base_line, kind, structural_path, language)];
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
                    language,
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
            language,
        ));
        if end == lines.len() {
            break;
        }
        let overlap = if matches!(kind, ChunkKind::Fallback | ChunkKind::Module) {
            OVERLAP_LINES
        } else {
            0
        };
        start = end.saturating_sub(overlap).max(start + 1);
    }
    chunks
}

fn empty_chunk(
    line: u32,
    kind: ChunkKind,
    structural_path: Option<String>,
    language: Option<&'static str>,
) -> ChunkDraft {
    draft(kind, line, line, String::new(), structural_path, language)
}

fn draft(
    kind: ChunkKind,
    line_start: u32,
    line_end: u32,
    body: String,
    structural_path: Option<String>,
    language: Option<&'static str>,
) -> ChunkDraft {
    ChunkDraft {
        kind,
        line_start,
        line_end,
        body,
        structural_path,
        language,
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

fn line_at_byte(text: &str, byte: usize) -> u32 {
    u32::try_from(text[..byte].bytes().filter(|value| *value == b'\n').count() + 1)
        .unwrap_or(u32::MAX)
}

fn point_line(point: Point) -> u32 {
    u32::try_from(point.row + 1).unwrap_or(u32::MAX)
}

fn end_line(node: Node<'_>) -> u32 {
    let end = node.end_position();
    let row = if end.column == 0 && end.row > node.start_position().row {
        end.row
    } else {
        end.row + 1
    };
    u32::try_from(row).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{ChunkKind, MAX_CHUNK_BYTES, chunk_text};
    use std::path::Path;

    #[test]
    fn markdown_sections_preserve_heading_paths_and_bounded_fallback_overlap() {
        let markdown =
            "preamble\n# Alpha\nalpha body\n```text\n# not a heading\n```\n## Beta\nbeta body\n";
        let chunks = chunk_text(Path::new("lesson.md"), markdown);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].kind, ChunkKind::Fallback);
        assert_eq!(chunks[1].structural_path.as_deref(), Some("Alpha"));
        assert_eq!(chunks[2].structural_path.as_deref(), Some("Alpha > Beta"));
        assert_eq!((chunks[2].line_start, chunks[2].line_end), (7, 8));

        let fallback = chunk_text(Path::new("opaque.txt"), &"x".repeat(MAX_CHUNK_BYTES + 5));
        assert_eq!(fallback.len(), 2);
        assert!(
            fallback
                .iter()
                .all(|chunk| chunk.body.len() <= MAX_CHUNK_BYTES)
        );
        let lines = (1..=130)
            .map(|line| format!("line {line}\n"))
            .collect::<String>();
        let fallback = chunk_text(Path::new("opaque.txt"), &lines);
        assert_eq!((fallback[0].line_start, fallback[0].line_end), (1, 120));
        assert_eq!(fallback[1].line_start, 113);
    }

    #[test]
    fn python_and_typescript_emit_symbol_import_and_reference_metadata() {
        let python = "from helpers import normalize\n\ndef clean(value):\n    return normalize(value)\n\nclass Runner:\n    def run(self, value):\n        return clean(value)\n";
        let python_chunks = chunk_text(Path::new("worker.py"), python);
        let clean = python_chunks
            .iter()
            .find(|chunk| chunk.structural_path.as_deref() == Some("clean"))
            .expect("Python function chunk");
        assert_eq!((clean.line_start, clean.line_end), (3, 4));
        assert!(clean.imports.is_empty());
        assert!(
            python_chunks
                .iter()
                .flat_map(|chunk| &chunk.imports)
                .any(|source_import| source_import.module == "helpers")
        );
        assert!(
            clean
                .references
                .iter()
                .any(|reference| reference.name == "normalize")
        );
        let runner = python_chunks
            .iter()
            .find(|chunk| chunk.structural_path.as_deref() == Some("Runner"))
            .expect("Python class chunk");
        assert!(
            runner
                .definitions
                .iter()
                .any(|definition| definition.name == "run")
        );

        let typescript = "import { parse } from './parse';\nexport function load(input: string) {\n  return parse(input);\n}\nexport class Store {\n  read() { return load('x'); }\n}\n";
        let typescript_chunks = chunk_text(Path::new("store.ts"), typescript);
        let load = typescript_chunks
            .iter()
            .find(|chunk| chunk.structural_path.as_deref() == Some("load"))
            .expect("TypeScript function chunk");
        assert!(load.imports.is_empty());
        assert!(
            typescript_chunks
                .iter()
                .flat_map(|chunk| &chunk.imports)
                .any(|source_import| source_import.module == "./parse")
        );
        assert!(
            load.references
                .iter()
                .any(|reference| reference.name == "parse")
        );
        assert!(
            typescript_chunks
                .iter()
                .any(|chunk| chunk.structural_path.as_deref() == Some("Store"))
        );

        let javascript = "import { parse } from './parse.js';\nfunction load(input) {\n  return parse(input);\n}\n";
        let javascript_chunks = chunk_text(Path::new("loader.js"), javascript);
        let load = javascript_chunks
            .iter()
            .find(|chunk| chunk.structural_path.as_deref() == Some("load"))
            .expect("JavaScript function chunk");
        assert_eq!(load.language, Some("javascript"));
        assert!(
            javascript_chunks
                .iter()
                .flat_map(|chunk| &chunk.imports)
                .any(|source_import| source_import.module == "./parse.js")
        );

        let imports_only = chunk_text(Path::new("imports.py"), "from helpers import normalize\n");
        assert_eq!(imports_only[0].kind, ChunkKind::Module);
        assert_eq!(imports_only[0].imports[0].module, "helpers");

        let long_name = "x".repeat(1_024);
        let source = format!(
            "from helpers import {long_name}\n\ndef alpha():\n    return 1\n\ndef beta():\n    return 2\n"
        );
        let chunks = chunk_text(Path::new("bounded.py"), &source);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.imports.len())
                .sum::<usize>(),
            1
        );
        assert!(
            chunks
                .iter()
                .flat_map(|chunk| &chunk.imports)
                .flat_map(|import| &import.names)
                .all(|name| name.len() <= super::MAX_METADATA_TEXT_BYTES)
        );
        assert!(chunks.iter().all(|chunk| {
            chunk
                .imports
                .iter()
                .map(|source_import| {
                    source_import.module.len()
                        + source_import.names.iter().map(String::len).sum::<usize>()
                })
                .sum::<usize>()
                <= super::MAX_IMPORT_TEXT_BYTES_PER_CHUNK
        }));
    }
}
