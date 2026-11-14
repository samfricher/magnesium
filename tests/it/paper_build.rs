//! The papers skill's LaTeX build, run as the tutor runs it: the templates
//! copied into `Study/<target>/papers/src/`, then `bash build.sh`.
//!
//! These tests need tectonic, which the tutor installs only when the learner
//! agrees. Without it on PATH they say so and pass.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::json;

use crate::support::{TestEnvironment, repo_root, v2};

const TEMPLATES: &str = "claude-plugin/skills/papers/templates";

/// The notation a tutor adds for a complexity course, as in the paper whose
/// two lines ran off the page.
const NOTATION: &str =
    "\\newcommand{\\lang}[1]{\\textsc{#1}}\n\\newcommand{\\enc}[1]{\\langle #1\\rangle}\n";

/// Lines 5 and 11 are the display from that paper, once in a question and
/// once in a solution. Lines 14 to 16 put a phrase too wide for a narrow box
/// at the right of the text, so it runs off the page. The box on line 18 is
/// never placed on a page, so nothing can tell where its line would end. The
/// rule is 60pt wider than the text, so it ends 12pt from the page's edge,
/// where a printer cuts it off. It is the last paragraph, which TeX sets only
/// after the body's last line.
const CUT: &str = r"\paperheader{Cut}{Medium}{50}{20}{none}

\question{1}{A reduction}
Let
\[\lang{MIN-SAT}=\{\enc{\Phi,k} : \Phi \text{ is a monotone CNF formula with a satisfying assignment that sets at most } k \text{ variables true}\}.\]
Show that it is in NP.
\begin{solution}
\solhead A certificate is the assignment. \mk{2}

The language again:
\[\lang{MIN-SAT}=\{\enc{\Phi,k} : \Phi \text{ is a monotone CNF formula with a satisfying assignment that sets at most } k \text{ variables true}\}.\]
\end{solution}

\hfill\begin{minipage}[t]{0.3\textwidth}
\mbox{An unbreakable phrase inside the right column}
\end{minipage}

\sbox0{\parbox{2cm}{\mbox{An unbreakable phrase}}}%
\noindent\rule{\dimexpr\textwidth+60pt\relax}{0.4pt}
";

/// Lines too wide for a narrow box at the left of the page: TeX's overrun is
/// measured from the box's edge, and neither line comes near the margin.
const MINI: &str = r"\paperheader{Mini}{Medium}{20}{10}{none}
\question{1}{Two columns}\pts{5}
\begin{minipage}[t]{0.3\textwidth}
\mbox{An unbreakable phrase inside the left column of two}
\end{minipage}\hfill
\begin{minipage}[t]{0.3\textwidth}
Right column.
\end{minipage}

\begin{tabular}{p{2cm}l}
\mbox{Wide cell content here} & fine \\
\end{tabular}
";

/// The same display rewritten as the papers skill says, and a rule 30pt past
/// the text, which stays on the page.
const FITS: &str = r"\paperheader{Fits}{Medium}{50}{20}{none}

\question{1}{A reduction}
Let
\begin{multline*}
\lang{MIN-SAT}=\{\enc{\Phi,k} : \Phi \text{ is a monotone CNF formula with a satisfying}\\
\text{assignment that sets at most } k \text{ variables true}\}.
\end{multline*}
Recall $\lang{VERTEX-COVER}=\{\enc{G,k}:$ $G$ has a vertex cover of size at most $k\}$.
\begin{solution}
\solhead A certificate is the assignment. \mk{2}
\end{solution}

\noindent\rule{\dimexpr\textwidth+30pt\relax}{0.4pt}

That is all.
";

fn tectonic_on_path() -> bool {
    Command::new("tectonic")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// A study folder's `papers/src/` with the template build and preamble, the
/// preamble with the course notation added under its notation comment.
fn papers_src(environment: &TestEnvironment) -> PathBuf {
    let src = environment.workspace.join("Study/cs-final/papers/src");
    fs::create_dir_all(&src).unwrap();
    let templates = repo_root().join(TEMPLATES);
    fs::copy(templates.join("build.sh"), src.join("build.sh")).unwrap();
    let preamble = fs::read_to_string(templates.join("preamble.tex")).unwrap();
    let marker = "% course's slides and textbook.\n";
    assert!(
        preamble.contains(marker),
        "the preamble template lost its notation comment"
    );
    fs::write(
        src.join("preamble.tex"),
        preamble.replacen(marker, &format!("{marker}{NOTATION}"), 1),
    )
    .unwrap();
    src
}

fn build(src: &Path) -> Output {
    Command::new("bash")
        .arg(src.join("build.sh"))
        .output()
        .expect("bash runs the build")
}

#[test]
fn the_paper_build_withholds_a_paper_with_a_line_off_the_page_and_says_where_each_line_ends() {
    if !tectonic_on_path() {
        eprintln!("skipped: tectonic is not on PATH");
        return;
    }
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    let src = papers_src(&environment);
    fs::write(src.join("Cut-body.tex"), CUT).unwrap();
    fs::write(src.join("Fits-body.tex"), FITS).unwrap();
    fs::write(src.join("Mini-body.tex"), MINI).unwrap();

    let output = build(&src);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let report = format!("stdout:\n{stdout}\nstderr:\n{stderr}");
    assert_eq!(output.status.code(), Some(1), "{report}");

    let papers = src.parent().unwrap();
    for (pdf, written) in [
        ("Cut.pdf", false),
        ("Cut-KEY.pdf", false),
        ("Fits.pdf", true),
        ("Fits-KEY.pdf", true),
        ("Mini.pdf", true),
        ("Mini-KEY.pdf", true),
    ] {
        assert_eq!(papers.join(pdf).exists(), written, "{pdf}\n{report}");
    }

    // The display and the narrow box at the right run past the page in the
    // paper and the key. The display in the solution is named by its line in
    // the body, though the comment package reads a solution from a file of
    // its own.
    for cut in [
        "CUT LINE in Cut.pdf: line 5 of Cut-body.tex is ",
        "CUT LINE in Cut-KEY.pdf: line 5 of Cut-body.tex is ",
        "CUT LINE in Cut-KEY.pdf: line 11 of Cut-body.tex is ",
        "CUT LINE in Cut.pdf: lines 15 to 16 of Cut-body.tex are ",
        "CUT LINE in Cut-KEY.pdf: lines 15 to 16 of Cut-body.tex are ",
    ] {
        let line = stderr
            .lines()
            .find(|line| line.starts_with(cut))
            .unwrap_or_else(|| panic!("no {cut:?}\n{report}"));
        assert!(line.ends_with("past the right edge of the page."), "{line}");
    }
    for unchecked in [
        "UNCHECKED LINE in Cut.pdf: line 18 of Cut-body.tex is ",
        "UNCHECKED LINE in Cut-KEY.pdf: line 18 of Cut-body.tex is ",
    ] {
        let line = stderr
            .lines()
            .find(|line| line.starts_with(unchecked))
            .unwrap_or_else(|| panic!("no {unchecked:?}\n{report}"));
        assert!(
            line.ends_with(
                "too wide for its column or cell, and build.sh cannot tell where on the page it ends."
            ),
            "{line}"
        );
    }
    for near in [
        "CUT LINE in Cut.pdf: lines 19 to the end of Cut-body.tex are 60.0pt too wide for their column, so they end 12.3pt from the right edge of the page, where a printer cuts them off.",
        "CUT LINE in Cut-KEY.pdf: lines 19 to the end of Cut-body.tex are 60.0pt too wide for their column, so they end 12.3pt from the right edge of the page, where a printer cuts them off.",
        "FAILED: Cut (paper): Cut.pdf was not written.",
        "FAILED: Cut (key): Cut-KEY.pdf was not written.",
    ] {
        assert!(
            stderr.lines().any(|line| line == near),
            "no {near:?}\n{report}"
        );
    }
    assert_eq!(stderr.matches("CUT LINE in").count(), 7, "{report}");
    assert_eq!(stderr.matches("UNCHECKED LINE in").count(), 2, "{report}");
    assert!(
        !stderr.contains("Fits") && !stderr.contains("Mini"),
        "{report}"
    );

    // The rewritten display breaks, and the rule stays on the page. The lines
    // too wide for their narrow boxes end inside the text, nowhere near the
    // margin, and are noted as such.
    for note in [
        "wide line in Fits.pdf: lines 14 to 15 of Fits-body.tex are 30.0pt too wide for their column, so they run 30.0pt into the right margin and stay on the page.",
        "wide line in Fits-KEY.pdf: lines 14 to 15 of Fits-body.tex are 30.0pt too wide for their column, so they run 30.0pt into the right margin and stay on the page.",
        "wide line in Mini.pdf: lines 4 to 5 of Mini-body.tex are 109.8pt too wide for their column but end 116.6pt short of the right margin. Check that they cover nothing beside them.",
        "wide line in Mini-KEY.pdf: lines 4 to 5 of Mini-body.tex are 109.8pt too wide for their column but end 116.6pt short of the right margin. Check that they cover nothing beside them.",
        "wide line in Mini.pdf: line 11 of Mini-body.tex is 49.9pt too wide for its column but ends 356.9pt short of the right margin. Check that it covers nothing beside it.",
        "wide line in Mini-KEY.pdf: line 11 of Mini-body.tex is 49.9pt too wide for its column but ends 356.9pt short of the right margin. Check that it covers nothing beside it.",
    ] {
        assert!(
            stdout.lines().any(|line| line == note),
            "no {note:?}\n{report}"
        );
    }
    assert_eq!(stdout.matches("wide line in").count(), 6, "{report}");
}

#[test]
fn the_papers_stage_and_skill_send_the_tutor_back_to_a_cut_line() {
    let environment = TestEnvironment::from_fixture("basic").unwrap();
    v2::init(&environment);
    let served = v2::tool(
        &environment,
        "claude",
        "cut-lines",
        "context",
        json!({"action":"instructions","host":"claude","stage":"papers"}),
    );
    let text = served["text"].as_str().unwrap();
    for rule in [
        "Never give the learner a paper or key with a line cut off at the edge of the page.",
        "When the build names such a line, rewrite it so it can break, and build again.",
    ] {
        assert!(text.contains(rule), "the papers stage lacks: {rule}");
    }
    // The skill tells an old build.sh from the template by the word the
    // template prints, so the two must keep the same word.
    let skill = include_str!("../../claude-plugin/skills/papers/SKILL.md");
    let script = include_str!("../../claude-plugin/skills/papers/templates/build.sh");
    assert!(
        skill.contains("`papers/src/build.sh` has no `CUT LINE` in it"),
        "{skill}"
    );
    assert!(script.contains("\"CUT LINE in %s: "), "{script}");
}
