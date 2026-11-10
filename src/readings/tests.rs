use super::*;
use crate::learner::practice::{ItemFacts, ItemState, Origin};
use chrono::NaiveDate;

const GRAPH: &str = r#"# Course graph: midterm-1
tags:
  chain-rule:
    covers: "Rule 3.1: derivative of a composite"
    pages: notes.pdf pages 4-6
    kind: course
    needs: [power-rule, "algebra"]
    key_need: [power-rule]
    contains: [power-rule]
    touches: [algebra]
  power-rule:
    covers: d/dx x^n
    kind: foundation
    needs: []
    key_need: []
    contains:
      - algebra   # fully exercised
    touches: []
  algebra:
    covers: rearranging
    kind: foundation
    needs: []
    key_need: []
    contains: []
    touches: []
"#;

#[test]
fn a_graph_is_read_in_flow_and_block_form_with_comments() {
    let graph = parse_graph(GRAPH).unwrap();
    assert_eq!(graph.order, ["chain-rule", "power-rule", "algebra"]);
    let chain = &graph.tags["chain-rule"];
    assert_eq!(chain.needs, ["power-rule", "algebra"]);
    assert_eq!(chain.key_need, ["power-rule"]);
    assert_eq!(graph.tags["power-rule"].contains, ["algebra"]);
}

#[test]
fn a_block_list_may_sit_at_its_key_indent_and_under_an_ignored_field() {
    let graph = parse_graph(
        "tags:\n  a:\n    pages:\n    - notes.pdf p4\n    - notes.pdf p9\n    needs:\n    - b\n    - c\n    key_need:\n      - b\n    touches:\n    - c\n  b:\n    contains: []\n  c:\n    needs: []\n",
    )
    .unwrap();
    assert_eq!(graph.order, ["a", "b", "c"]);
    let a = &graph.tags["a"];
    assert_eq!(a.needs, ["b", "c"]);
    assert_eq!(a.key_need, ["b"]);
    assert!(a.contains.is_empty());
}

#[test]
fn the_shipped_template_is_read() {
    let graph = parse_graph(include_str!(
        "../../claude-plugin/skills/papers/templates/graph.yaml"
    ))
    .unwrap();
    assert_eq!(graph.order, ["<tag>", "<foundation_tag>"]);
    let tag = &graph.tags["<tag>"];
    assert_eq!(tag.needs, ["<foundation_tag>"]);
    assert_eq!(tag.key_need, ["<foundation_tag>"]);
    assert_eq!(tag.contains, ["<foundation_tag>"]);
}

#[test]
fn a_graph_it_cannot_be_sure_of_is_refused_with_its_line() {
    let error = parse_graph("tags:\n  a:\n    needs: b, c\n").unwrap_err();
    assert!(error.starts_with("line 3:"), "{error}");
    assert!(
        parse_graph("tags:\n  a:\n  a:\n")
            .unwrap_err()
            .contains("twice")
    );
    assert!(parse_graph("title: x\n").is_err());
    assert!(parse_graph("tags:\n\ta:\n").is_err());
}

fn row(plan: &str, n: i64, tag: &str, passes: i64, next_on: Option<NaiveDate>) -> Item {
    let day = NaiveDate::from_ymd_opt(2026, 11, 1).unwrap();
    Item {
        facts: ItemFacts {
            item_id: format!("g-{n}"),
            origin: Origin::Paper,
            label: tag.into(),
            plan: Some(plan.into()),
            row_no: Some(n),
            paper: Some("MockA".into()),
            paper_question: Some("1".into()),
            logged_on: day,
            gap_type: None,
            what_went_wrong: String::new(),
            repair: String::new(),
            confidence: None,
            next_question: None,
            answer_key: None,
            dropped: None,
            base_passes: 0,
            base_next_on: None,
            base_last_pass_on: None,
            base_history: String::new(),
            base_misses: Vec::new(),
            created_at_ms: 0,
            events: Vec::new(),
        },
        state: ItemState {
            passes,
            next_on,
            history: Vec::new(),
            confidence: None,
            misses: Vec::new(),
            effects: Vec::new(),
        },
    }
}

fn attempt(tag: &str, right: bool, cold: bool, slow: bool) -> TagAttempt {
    TagAttempt {
        label_key: tag.into(),
        right,
        partly: false,
        alone: true,
        cold,
        slow,
        place: "MockA Q2".into(),
    }
}

fn readings(items: &[Item], attempts: &[TagAttempt]) -> BTreeMap<String, (Reading, String)> {
    let graph = parse_graph(GRAPH).unwrap();
    read_plan(&PlanInputs {
        plan: "midterm-1",
        graph: Some(&graph),
        items,
        attempts,
        imported: &BTreeMap::new(),
    })
    .into_iter()
    .map(|r| (r.tag, (r.reading, r.why)))
    .collect()
}

#[test]
fn with_no_evidence_every_tag_is_untested() {
    let read = readings(&[], &[]);
    assert!(
        read.values().all(|(r, _)| *r == Reading::Untested),
        "{read:?}"
    );
}

#[test]
fn a_cold_pass_is_sound_for_the_tag_and_what_it_contains() {
    let read = readings(&[], &[attempt("chain-rule", true, true, false)]);
    assert_eq!(
        read["chain-rule"],
        (Reading::Sound, "cold in MockA Q2".into())
    );
    assert_eq!(
        read["power-rule"],
        (Reading::Sound, "from chain-rule via contains".into())
    );
    assert_eq!(
        read["algebra"],
        (
            Reading::Sound,
            "inferred from chain-rule via contains".into()
        )
    );
}

#[test]
fn a_slow_or_helped_pass_is_conditional_and_goes_nowhere() {
    let read = readings(&[], &[attempt("chain-rule", true, true, true)]);
    assert_eq!(read["chain-rule"].0, Reading::Conditional);
    assert!(read["chain-rule"].1.contains("slow"));
    assert_eq!(read["power-rule"].0, Reading::Untested);
    let read = readings(&[], &[attempt("chain-rule", true, false, false)]);
    assert_eq!(read["chain-rule"].0, Reading::Conditional);
}

#[test]
fn a_reading_follows_its_gap_row_and_its_key_need() {
    let day = NaiveDate::from_ymd_opt(2026, 11, 12);
    let passed = [attempt("chain-rule", true, true, false)];
    // An open row on power-rule: it reads open, and chain-rule, which has it
    // in key_need, reads conditional even with its own cold pass.
    let read = readings(&[row("midterm-1", 1, "power-rule", 0, None)], &passed);
    assert_eq!(
        read["power-rule"],
        (
            Reading::Open,
            "midterm-1#1 open, waiting for its repair".into()
        )
    );
    assert_eq!(
        read["chain-rule"],
        (
            Reading::Conditional,
            "needs power-rule, which is open".into()
        )
    );
    // The row passes once: conditional.
    let read = readings(&[row("midterm-1", 1, "power-rule", 1, day)], &passed);
    assert_eq!(
        read["power-rule"],
        (Reading::Conditional, "midterm-1#1 at 1 of 3".into())
    );
    assert_eq!(read["chain-rule"].0, Reading::Sound);
    // Closed: sound.
    let read = readings(&[row("midterm-1", 1, "power-rule", 3, None)], &[]);
    assert_eq!(
        read["power-rule"],
        (Reading::Sound, "midterm-1#1 closed".into())
    );
    // A row in another plan changes nothing here.
    let read = readings(&[row("final", 1, "power-rule", 0, None)], &[]);
    assert_eq!(read["power-rule"].0, Reading::Untested);
}

#[test]
fn an_open_tag_in_needs_holds_a_cold_pass_at_conditional() {
    let read = readings(
        &[row("midterm-1", 1, "algebra", 0, None)],
        &[attempt("chain-rule", true, true, false)],
    );
    assert_eq!(
        read["chain-rule"],
        (Reading::Conditional, "needs algebra, which is open".into())
    );
}

#[test]
fn a_row_on_a_tag_the_graph_lacks_is_still_read() {
    let read = readings(&[row("midterm-1", 1, "limits", 0, None)], &[]);
    assert_eq!(read["limits"].0, Reading::Open);
}

#[test]
fn a_reading_brought_in_from_a_memory_counts_as_evidence() {
    let graph = parse_graph(GRAPH).unwrap();
    let imported = BTreeMap::from([(
        "algebra".to_owned(),
        ("sound".to_owned(), "sound from Diagnostic Q1".to_owned()),
    )]);
    let read = read_plan(&PlanInputs {
        plan: "midterm-1",
        graph: Some(&graph),
        items: &[],
        attempts: &[],
        imported: &imported,
    });
    let algebra = read.iter().find(|r| r.tag == "algebra").unwrap();
    assert_eq!(algebra.reading, Reading::Sound);
    assert!(algebra.why.contains("Diagnostic Q1"));
}
