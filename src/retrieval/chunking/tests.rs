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

fn headings(name: &str, text: &str) -> Vec<(u32, u32, Option<String>, ChunkKind)> {
    chunk_text(Path::new(name), text)
        .into_iter()
        .map(|chunk| {
            (
                chunk.line_start,
                chunk.line_end,
                chunk.structural_path,
                chunk.kind,
            )
        })
        .collect()
}

#[test]
fn slide_sources_use_markdown_headings_without_their_attributes() {
    let qmd = "---\ntitle: \"Lecture 8\"\n---\n\n# Recurrent Networks\n\n## Parity {.smaller}\n\n```{python}\n# not a heading\nx = 1\n```\n\n## Parity\n\ntext\n";
    for name in ["lec08.qmd", "notes.Rmd"] {
        let found = headings(name, qmd);
        assert_eq!(
            found
                .iter()
                .map(|(start, _, path, _)| (*start, path.clone()))
                .collect::<Vec<_>>(),
            [
                (1, None),
                (5, Some("Recurrent Networks".into())),
                (7, Some("Recurrent Networks > Parity".into())),
                (14, Some("Recurrent Networks > Parity".into())),
            ],
            "{name}"
        );
        assert!(
            found[1..]
                .iter()
                .all(|(_, _, _, kind)| *kind == ChunkKind::Heading)
        );
    }
}

#[test]
fn web_pages_use_their_heading_tags_and_skip_scripts() {
    let html = "<html><head><style>h2 { color: red }</style>\n<script>var s = '<h2>fake</h2>';</script></head>\n<body>\n<h1 class=\"title\">10.1. Long Short-Term Memory (LSTM)<a class=\"headerlink\" href=\"#lstm\">¶</a></h1>\n<p>intro</p>\n<section><h2 id=\"gates\">Gated Memory &amp; Cells</h2>\n<p>gates</p>\n<h3\n>split tag</h3>\n<h2>Summary</h2>\n</body></html>\n";
    let found = headings("lstm.html", html);
    assert_eq!(
        found
            .iter()
            .map(|(start, end, path, _)| (*start, *end, path.clone()))
            .collect::<Vec<_>>(),
        [
            (1, 3, None),
            (4, 5, Some("10.1. Long Short-Term Memory (LSTM)".into())),
            (
                6,
                7,
                Some("10.1. Long Short-Term Memory (LSTM) > Gated Memory & Cells".into())
            ),
            (
                8,
                9,
                Some(
                    "10.1. Long Short-Term Memory (LSTM) > Gated Memory & Cells > split tag".into()
                )
            ),
            (
                10,
                11,
                Some("10.1. Long Short-Term Memory (LSTM) > Summary".into())
            ),
        ]
    );
}

#[test]
fn latex_uses_sectioning_commands_and_frame_titles() {
    let tex = "\\documentclass{beamer}\n\\section{Estimation}\n% \\section{commented out}\n\\begin{frame}[t]{Pivotal \\textbf{quantity}}\nbody\n\\end{frame}\n\\begin{frame}\n\\frametitle{Exercise 8.59}\n\\end{frame}\n\\subsection*{Bias~and MSE\\label{sec:bias}}\n\\sectionmark{not a section}\n";
    let found = headings("chap8.tex", tex);
    assert_eq!(
        found
            .iter()
            .map(|(start, _, path, _)| (*start, path.clone()))
            .collect::<Vec<_>>(),
        [
            (1, None),
            (2, Some("Estimation".into())),
            (4, Some("Estimation > Pivotal quantity".into())),
            (8, Some("Estimation > Exercise 8.59".into())),
            (10, Some("Estimation > Bias and MSE".into())),
        ]
    );
}

#[test]
fn notebook_cells_keep_the_lines_of_the_original_file_and_carry_headings_across_cells() {
    let notebook = r###"{
 "cells": [
  {
   "cell_type": "markdown",
   "metadata": {},
   "source": [
    "# Lab 10\n",
    "\n",
    "## Part 1: an RNN by hand\n",
    "Some \"quoted\" words."
   ]
  },
  {
   "cell_type": "code",
   "execution_count": 1,
   "metadata": {},
   "outputs": [],
   "source": [
    "import torch\n",
    "# a comment, not a heading\n",
    "h = torch.zeros(3)"
   ]
  }
 ],
 "metadata": {},
 "nbformat": 4,
 "nbformat_minor": 5
}
"###;
    let chunks = chunk_text(Path::new("lab10.ipynb"), notebook);
    let found = chunks
        .iter()
        .map(|chunk| {
            (
                chunk.line_start,
                chunk.line_end,
                chunk.structural_path.clone(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        found,
        [
            (7, 8, Some("Lab 10".into())),
            (9, 10, Some("Lab 10 > Part 1: an RNN by hand".into())),
            (19, 21, Some("Lab 10 > Part 1: an RNN by hand".into())),
        ]
    );
    let lines = notebook.lines().collect::<Vec<_>>();
    assert!(lines[8].contains("Part 1"));
    assert!(lines[18].contains("import torch"));
    assert!(chunks[1].body.contains("Some \"quoted\" words."));
    assert!(chunks[2].body.contains("h = torch.zeros(3)"));
    assert!(!chunks[2].body.contains("\\n"));

    let minified = notebook.replace('\n', "");
    let fallback = chunk_text(Path::new("lab10.ipynb"), &minified);
    assert!(
        fallback
            .iter()
            .all(|chunk| chunk.kind == ChunkKind::Fallback && chunk.structural_path.is_none())
    );
}

#[test]
fn only_the_first_chunk_of_a_long_section_starts_it() {
    let body = (0..300)
        .map(|line| format!("line {line}\n"))
        .collect::<String>();
    let markdown = format!("# Long\n{body}# Long\nshort\n");
    let found = headings("long.md", &markdown);
    let kinds = found
        .iter()
        .map(|(_, _, path, kind)| (path.as_deref() == Some("Long"), *kind))
        .collect::<Vec<_>>();
    assert_eq!(kinds[0], (true, ChunkKind::Heading));
    assert!(
        kinds[1..kinds.len() - 1]
            .iter()
            .all(|kind| *kind == (true, ChunkKind::Fallback))
    );
    assert_eq!(kinds.last(), Some(&(true, ChunkKind::Heading)));
}
