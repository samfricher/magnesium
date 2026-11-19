const ORCHESTRATOR: &str = include_str!("../workspace/learning/MAGNESIUM.md");
const HEADING: &str = "## Core rules\n\n";

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
            "**A task stands alone.**",
            "**Start from one concrete case.**",
            "**An answer counts only when it rules out the misunderstanding.**",
            "**Done means done alone, on a later day.**",
            "**A plan request ends with a task**",
        ] {
            assert!(core.contains(rule), "{rule}");
        }
        assert!(core.len() < 2304, "{} bytes", core.len());
    }
}
