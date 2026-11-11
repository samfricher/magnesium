use super::*;
#[test]
fn fresh_config_holds_only_its_schema_and_local_identity() {
    let root = std::env::temp_dir().join(format!(
        "magnesium-config-{}",
        crate::workspace::os_random_hex(16).unwrap()
    ));
    fs::create_dir(&root).unwrap();
    let path = root.join("config.toml");
    let mut config = WorkspaceConfig::new(path.clone(), "workspace-a").unwrap();
    config.save().unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "schema_version = 1\nworkspace_id = \"workspace-a\"\n"
    );
    assert_eq!(
        WorkspaceConfig::load(&path).unwrap().workspace_id(),
        "workspace-a"
    );
    fs::remove_dir_all(root).unwrap();
}

/// The papers skill records optional-tool answers in a `[tools]` table.
/// `magnesium init` only loads the config; a load and save must still keep it.
#[test]
fn a_tools_table_survives_load_and_save() {
    let root = std::env::temp_dir().join(format!(
        "magnesium-config-{}",
        crate::workspace::os_random_hex(16).unwrap()
    ));
    fs::create_dir(&root).unwrap();
    let path = root.join("config.toml");
    fs::write(
        &path,
        "schema_version = 1\nworkspace_id = \"workspace-a\"\n\n[tools]\ntectonic = \"declined\"\n",
    )
    .unwrap();
    let mut config = WorkspaceConfig::load(&path).unwrap();
    assert_eq!(config.workspace_id(), "workspace-a");
    config.save().unwrap();
    let saved: DocumentMut = fs::read_to_string(&path).unwrap().parse().unwrap();
    assert_eq!(saved["tools"]["tectonic"].as_str(), Some("declined"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_core_rules_switch_sits_above_the_tools_table_and_turns_back_on() {
    let root = std::env::temp_dir().join(format!(
        "magnesium-config-{}",
        crate::workspace::os_random_hex(16).unwrap()
    ));
    fs::create_dir(&root).unwrap();
    let path = root.join("config.toml");
    fs::write(
        &path,
        "schema_version = 1\nworkspace_id = \"workspace-a\"\n\n[tools]\ntectonic = \"declined\"\n",
    )
    .unwrap();
    let mut config = WorkspaceConfig::load(&path).unwrap();
    assert!(config.core_rules());
    config.set_core_rules(false);
    config.save().unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.find("core_rules = false").unwrap() < text.find("[tools]").unwrap(),
        "{text}"
    );
    let mut config = WorkspaceConfig::load(&path).unwrap();
    assert!(!config.core_rules());
    assert_eq!(
        config.document["tools"]["tectonic"].as_str(),
        Some("declined")
    );
    config.set_core_rules(true);
    config.save().unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains("core_rules"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_opener_switch_has_three_states_and_on_is_the_default() {
    let root = std::env::temp_dir().join(format!(
        "magnesium-config-{}",
        crate::workspace::os_random_hex(16).unwrap()
    ));
    fs::create_dir(&root).unwrap();
    let path = root.join("config.toml");
    fs::write(
            &path,
            "schema_version = 1\nworkspace_id = \"workspace-a\"\nopener = \"sometimes\"\n\n[tools]\ntectonic = \"declined\"\n",
        )
        .unwrap();
    let mut config = WorkspaceConfig::load(&path).unwrap();
    assert_eq!(config.opener(), OpenerMode::On, "an unknown word means on");
    for mode in [OpenerMode::Off, OpenerMode::Block] {
        config.set_opener(mode);
        config.save().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.find("opener = ").unwrap() < text.find("[tools]").unwrap(),
            "{text}"
        );
        config = WorkspaceConfig::load(&path).unwrap();
        assert_eq!(config.opener(), mode);
    }
    config.set_opener(OpenerMode::On);
    config.save().unwrap();
    assert!(!fs::read_to_string(&path).unwrap().contains("opener"));
    fs::remove_dir_all(root).unwrap();
}
