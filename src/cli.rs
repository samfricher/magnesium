use std::path::PathBuf;

use clap::{Arg, ArgAction, Command as ClapCommand, value_parser};

const PUBLIC_COMMANDS: [(&str, &str); 9] = [
    ("init", "Initialize Magnesium in a workspace"),
    (
        "update",
        "Refresh installed host plugins and show how to update Magnesium",
    ),
    ("info", "Report passive Magnesium status"),
    (
        "digest",
        "Summarize this study folder and its learner record without changing them",
    ),
    (
        "attempts",
        "List every answer the tutor marked, with its question, mark, date, help and confidence",
    ),
    ("export", "Export a readable scope-safe learner-data copy"),
    ("memory", "Maintain workspace learner-memory search"),
    (
        "core-rules",
        "Turn on or off the core teaching rules the runtime restates after a compaction",
    ),
    (
        "opener",
        "Turn on or off the quick checks and paper redos that open each sitting",
    ),
];

pub(crate) fn command() -> ClapCommand {
    let public = PUBLIC_COMMANDS.into_iter().fold(
        ClapCommand::new("magnesium")
            .version(env!("CARGO_PKG_VERSION"))
            .about("Local workspace-attached learning support for the individual human learner")
            .disable_help_subcommand(true)
            .arg_required_else_help(true),
        |command, (name, about)| {
            let subcommand = ClapCommand::new(name).about(about);
            let subcommand = if name == "init" {
                subcommand
                    .arg(
                        Arg::new("path")
                            .value_name("PATH")
                            .value_parser(value_parser!(PathBuf)),
                    )
                    .arg(
                        Arg::new("adapter")
                            .long("adapter")
                            .value_name("ADAPTER")
                            .help("Configure optional adapter bootstrap (e.g. agents-md)"),
                    )
                    .arg(
                        Arg::new("json")
                            .long("json")
                            .help("Emit stable machine-readable workspace status")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(
                        Arg::new("upgrade-instructions")
                            .long("upgrade-instructions")
                            .help("Switch a workspace set up by an earlier Magnesium version to this version's teaching instructions; unedited earlier instructions are kept on disk")
                            .action(ArgAction::SetTrue),
                    )
            } else if name == "info" || name == "attempts" {
                subcommand.arg(
                    Arg::new("json")
                        .long("json")
                        .help(if name == "info" {
                            "Print the status as JSON on one line instead of pretty-printed"
                        } else {
                            "Print the answers as JSON instead of text"
                        })
                        .action(ArgAction::SetTrue),
                )
            } else if name == "export" {
                subcommand
                    .arg(
                        Arg::new("destination")
                            .value_name("DESTINATION")
                            .value_parser(value_parser!(PathBuf))
                            .required(true),
                    )
                    .arg(
                        Arg::new("format")
                            .long("format")
                            .value_name("FORMAT")
                            .value_parser(["dir", "zip"])
                            .default_value("dir"),
                    )
                    .arg(
                        Arg::new("include-source-copies")
                            .long("include-source-copies")
                            .help("Copy verified in-scope source files (default: references only)")
                            .action(ArgAction::SetTrue),
                    )
                    .arg(
                        Arg::new("overwrite")
                            .long("overwrite")
                            .help("Replace an existing destination")
                            .action(ArgAction::SetTrue),
                    )
            } else if name == "core-rules" {
                subcommand.arg(
                    Arg::new("state")
                        .value_name("STATE")
                        .value_parser(["on", "off"])
                        .required(true)
                        .help("off asks the tutor to reload its instructions after a compaction instead, as before"),
                )
            } else if name == "opener" {
                subcommand.arg(
                    Arg::new("state")
                        .value_name("STATE")
                        .value_parser(["on", "off", "block"])
                        .required(true)
                        .help("block also refuses new teaching stages until each quick check is posed or declined"),
                )
            } else if name == "memory" {
                subcommand.subcommand_required(true)
                    .subcommand(ClapCommand::new("reindex").about("Rebuild local learner-memory FTS indexes"))
            } else {
                subcommand
            };
            command.subcommand(subcommand)
        },
    );

    public
        .subcommand(
            ClapCommand::new("adapter")
                .hide(true)
                .subcommand_required(true)
                .subcommand(
                    ClapCommand::new("call")
                        .arg(
                            Arg::new("operation")
                                .required(true)
                                .value_parser(crate::mcp::catalog::CANONICAL_OPERATIONS),
                        )
                        .arg(
                            Arg::new("protocol-version")
                                .long("protocol-version")
                                .required(true)
                                .value_parser(value_parser!(u64)),
                        )
                        .arg(
                            Arg::new("json-input")
                                .long("json-input")
                                .required(true)
                                .value_name("FILE_OR_DASH"),
                        ),
                ),
        )
        .subcommand(
            ClapCommand::new("_internal")
                .hide(true)
                .subcommand_required(true)
                .subcommand(
                    ClapCommand::new("mcp-server")
                        .arg(Arg::new("plugin-version").long("plugin-version"))
                        .arg(Arg::new("adapter-protocol-version").long("adapter-protocol-version"))
                        .arg(
                            Arg::new("host-kind")
                                .long("host-kind")
                                .value_name("HOST_KIND")
                                .help("Host whose plugin launched this server; only claude is supported"),
                        ),
                )
                .subcommand(ClapCommand::new("extract-pdf"))
                .subcommand(
                    ClapCommand::new("hook")
                        .arg(Arg::new("plugin-version").long("plugin-version"))
                        .arg(Arg::new("adapter-protocol-version").long("adapter-protocol-version"))
                        .arg(
                            Arg::new("host-kind")
                                .long("host-kind")
                                .value_name("HOST_KIND")
                                .help("Host whose plugin runs this hook, because its event JSON carries no kind field; only claude is supported"),
                        )
                        // The retired events still parse; see hooks::RETIRED_EVENTS.
                        .arg(Arg::new("event").required(true).value_parser([
                            "session-start",
                            "user-prompt-submit",
                            "stop",
                            "session-end",
                            crate::hooks::RETIRED_EVENTS[0],
                            crate::hooks::RETIRED_EVENTS[1],
                        ])),
                ),
        )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_bare_invocation_shows_the_help() {
        let error = super::command()
            .try_get_matches_from(["magnesium"])
            .expect_err("a bare invocation is not a command");
        assert_eq!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
        assert!(error.to_string().contains("Commands:"), "{error}");
    }
}
