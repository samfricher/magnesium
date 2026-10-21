//! Refresh the installed plugin copies for this runtime and print the steps
//! that update the runtime itself. Magnesium is built from source, so this
//! command never fetches or runs anything over the network.
//!
//! Learner stores belong to individual workspaces. Updating the installation
//! never opens, migrates, imports, or deletes learner databases.

use std::path::PathBuf;
use std::process::ExitCode;

use crate::workspace::GlobalPaths;

/// The update steps of docs/install.md. Magnesium cannot rebuild itself.
const SOURCE_UPDATE_STEPS: &str = "Magnesium is built from source. To update it (docs/install.md, \"Update\"):
  1. In your clone of the Magnesium repository, run `git fetch --tags`, find the newest release tag with `git tag --no-column --sort=-v:refname | grep -Ex 'v[0-9]+\\.[0-9]+\\.[0-9]+' | head -1`, and run `git checkout <newest-tag>`. A detached HEAD is expected. Check that `git tag --no-column --points-at HEAD --sort=-v:refname | grep -Ex 'v[0-9]+\\.[0-9]+\\.[0-9]+' | head -1` prints the tag that the lookup printed. Empty output means the commit has no release tag.
  2. Run `cargo build --locked --release`.
  3. Put the new `target/release/magnesium` on your PATH in place of the old one, then check that `magnesium --version` prints the tag's version.
  4. In each study folder, run `magnesium init --upgrade-instructions`, then type `/magnesium:help` in a new Claude Code conversation.
If docs/install.md in the checked-out release gives other steps, follow it.";

pub(crate) fn run() -> ExitCode {
    println!("{SOURCE_UPDATE_STEPS}");
    let paths = match GlobalPaths::resolve() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("Magnesium update unavailable: {error}");
            return ExitCode::from(1);
        }
    };
    if !paths.installed() {
        println!("No Magnesium plugins are installed yet. Run `magnesium init` in a study folder.");
        return ExitCode::SUCCESS;
    }

    let mut failed_count = 0;
    if let Err(error) = crate::workspace::materialize_bundled_marketplace(&paths.marketplace()) {
        failed_count += 1;
        eprintln!("Failed to refresh plugin assets: {error}");
    } else {
        println!("Plugin assets: refreshed bundled marketplace assets.");
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if let Err(error) =
        crate::workspace::refresh_and_verify_codex_plugin(&cwd, &paths.marketplace())
    {
        failed_count += 1;
        eprintln!("Failed to refresh and verify Codex plugin: {error}");
    }
    match crate::workspace::refresh_claude_plugin(&paths.claude_plugin()) {
        Ok(true) => println!("Claude Code plugin: refreshed the plugin copy."),
        Ok(false) => {}
        Err(error) => {
            failed_count += 1;
            eprintln!("Failed to refresh the Claude Code plugin: {error}");
        }
    }
    crate::workspace::install_opencode_adapter().print();
    if failed_count == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}
