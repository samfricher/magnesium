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
fn markdown_heading_paths_follow_levels_when_levels_skip() {
    let paths = |markdown: &str| {
        chunk_text(Path::new("lesson.md"), markdown)
            .into_iter()
            .map(|chunk| chunk.structural_path)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        paths("## A\na\n## B\nb\n## C\nc\n"),
        [Some("A".into()), Some("B".into()), Some("C".into())]
    );
    assert_eq!(
        paths("# T\nt\n### x\nx\n### y\ny\n"),
        [Some("T".into()), Some("T > x".into()), Some("T > y".into())]
    );
    assert_eq!(
        paths("# T\nt\n## A\na\n### x\nx\n## B\nb\n# U\nu\n"),
        [
            Some("T".into()),
            Some("T > A".into()),
            Some("T > A > x".into()),
            Some("T > B".into()),
            Some("U".into()),
        ]
    );
}

#[test]
fn long_lines_keep_overlap_to_a_small_share_of_each_chunk() {
    let line = format!("{}\n", "a".repeat(1_999));
    let text = line.repeat(40);
    let chunks = chunk_text(Path::new("long.txt"), &text);
    let stored = chunks.iter().map(|chunk| chunk.body.len()).sum::<usize>();
    assert!(
        stored <= text.len() * 2,
        "{} chunks store {stored} bytes for {} input bytes",
        chunks.len(),
        text.len()
    );
    assert_eq!(chunks.last().map(|chunk| chunk.line_end), Some(40));

    let text = format!(
        "{}{}\n",
        "short\n".repeat(8),
        "b".repeat(MAX_CHUNK_BYTES + 1)
    );
    let chunks = chunk_text(Path::new("long.txt"), &text);
    let short = chunks
        .iter()
        .filter(|chunk| chunk.body.starts_with("short"))
        .count();
    assert_eq!(short, 1, "short lines before a long line were repeated");
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

    let javascript =
        "import { parse } from './parse.js';\nfunction load(input) {\n  return parse(input);\n}\n";
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
