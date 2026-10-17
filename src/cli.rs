use std::path::PathBuf;

use clap::{Arg, ArgAction, Command as ClapCommand, value_parser};

const PUBLIC_COMMANDS: [(&str, &str); 6] = [
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
    ("export", "Export a readable scope-safe learner-data copy"),
    ("memory", "Maintain workspace learner-memory search"),
];

pub(crate) fn command() -> ClapCommand {
    let public = PUBLIC_COMMANDS.into_iter().fold(
        ClapCommand::new("magnesium")
            .version(env!("CARGO_PKG_VERSION"))
            .about("Local workspace-attached learning support for the individual human learner")
            .disable_help_subcommand(true),
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
            } else if name == "info" {
                subcommand.arg(
                    Arg::new("json")
                        .long("json")
                        .help("Emit stable machine-readable status")
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
                                .help("Host whose plugin launched this server; only its session identity is trusted (e.g. claude)"),
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
                                .help("Adapter host kind for hosts whose native event JSON carries no kind field (e.g. claude)"),
                        )
                        .arg(Arg::new("event").required(true).value_parser([
                            "session-start",
                            "user-prompt-submit",
                            "stop",
                            "pre-compact",
                            "post-compact",
                            "session-end",
                        ])),
                ),
        )
}
