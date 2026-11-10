//! The core teaching rules the runtime puts in front of the tutor itself.
//!
//! A host compaction drops earlier tool results, and with them the stage
//! instructions. The runtime restates this short core instead of asking the
//! tutor to reload. The words are the `## Core rules` section of the served
//! `MAGNESIUM.md`, so every stage load and every restatement agree.

const ORCHESTRATOR: &str = include_str!("../workspace/learning/MAGNESIUM.md");
const HEADING: &str = "## Core rules\n\n";

/// The section, from its heading to the next heading.
pub(crate) fn text() -> &'static str {
    let start = ORCHESTRATOR
        .find(HEADING)
        .expect("MAGNESIUM.md has a core rules section");
    let section = &ORCHESTRATOR[start..];
    let end = section[HEADING.len()..]
        .find("\n## ")
        .map_or(section.len(), |offset| offset + HEADING.len());
    section[..end].trim_end()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_core_is_one_section_of_the_served_orchestrator() {
        let core = super::text();
        assert!(core.starts_with("## Core rules\n\n"), "{core}");
        assert!(!core["## Core rules".len()..].contains("\n## "), "{core}");
        for rule in [
            "**Each turn**",
            "**Start from one concrete case.**",
            "**An answer counts only when it rules out the misunderstanding.**",
            "**Done means done alone, on a later day.**",
            "**A plan request ends with a task**",
        ] {
            assert!(core.contains(rule), "{rule}");
        }
        assert!(core.len() < 2048, "{} bytes", core.len());
    }
}
