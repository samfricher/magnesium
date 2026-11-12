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
fn code_files_split_as_plain_text() {
    let python =
        "from helpers import normalize\n\ndef clean(value):\n    return normalize(value)\n";
    for name in ["worker.py", "store.ts", "loader.js", "view.tsx"] {
        let chunks = chunk_text(Path::new(name), python);
        assert_eq!(chunks.len(), 1, "{name}");
        let chunk = &chunks[0];
        assert_eq!(chunk.kind, ChunkKind::Fallback, "{name}");
        assert_eq!((chunk.line_start, chunk.line_end), (1, 4), "{name}");
        assert_eq!(chunk.body, python, "{name}");
        assert_eq!(chunk.structural_path, None, "{name}");
        assert_eq!(chunk.language, None, "{name}");
        assert!(
            chunk.definitions.is_empty() && chunk.imports.is_empty(),
            "{name}"
        );
        assert!(chunk.references.is_empty(), "{name}");
    }
}
