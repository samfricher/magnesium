use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::claude::clip;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ask {
    pub title: String,
    pub detail: String,
    pub exact: Option<String>,
}

fn ask(title: &str, detail: impl Into<String>) -> Ask {
    Ask {
        title: format!("The tutor asks to {title}"),
        detail: detail.into(),
        exact: None,
    }
}

enum Place {
    Plan(String),
    Course(String),
    Outside(String, String),
}

fn roots(folder: &Path) -> Vec<PathBuf> {
    let mut all = vec![crate::direct::study_root(folder), folder.to_path_buf()];
    for each in all.clone() {
        if let Ok(real) = std::fs::canonicalize(&each) {
            all.push(real);
        }
    }
    all.dedup();
    all
}

fn resolve(path: &Path) -> Option<PathBuf> {
    let mut rest = Vec::new();
    for base in path.ancestors() {
        if let Ok(real) = std::fs::canonicalize(base) {
            return Some(rest.iter().rev().fold(real, |at, part| at.join(part)));
        }
        rest.push(base.file_name()?.to_os_string());
    }
    None
}

fn inside(path: &Path, folder: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        folder.join(path)
    };
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir))
    {
        return None;
    }
    let real = resolve(&path)?;
    let root = crate::direct::study_root(folder);
    let real_root = std::fs::canonicalize(&root).ok()?;
    real.strip_prefix(&real_root).ok().map(Path::to_path_buf)
}

fn rule_folder(glob: &str, folder: &Path) -> Option<PathBuf> {
    let dir = glob.strip_suffix("/**")?;
    Some(if let Some(absolute) = dir.strip_prefix("//") {
        PathBuf::from(format!("/{absolute}"))
    } else if let Some(home) = dir.strip_prefix("~/") {
        crate::paths::home().join(home)
    } else {
        folder.join(dir.trim_start_matches('/'))
    })
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn named_with_folder(path: &Path) -> String {
    let name = name_of(path);
    match path
        .parent()
        .map(name_of)
        .filter(|folder| !folder.is_empty())
    {
        Some(folder) => format!("{name}, in {folder}"),
        None => name,
    }
}

fn title_of_target(target: &str) -> String {
    let words = target.replace(['-', '_'], " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn plan_of(relative: &Path, folder: &Path) -> Option<String> {
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let [study, target, file] = parts.as_slice() else {
        return None;
    };
    if study != "Study" || file != "plan.md" {
        return None;
    }
    let titled = crate::runtime::digest(folder).ok().and_then(|digest| {
        digest["plans"].as_array()?.iter().find_map(|plan| {
            let path = Path::new(plan["path"].as_str()?);
            (path.parent() == relative.parent())
                .then(|| {
                    plan["title"]
                        .as_str()
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                })
                .flatten()
                .map(str::to_owned)
        })
    });
    Some(titled.unwrap_or_else(|| title_of_target(target)))
}

fn place(path: &str, folder: &Path) -> Place {
    let path = Path::new(path);
    match inside(path, folder) {
        Some(relative) => match plan_of(&relative, folder) {
            Some(plan) => Place::Plan(plan),
            None => Place::Course(name_of(&relative)),
        },
        None => {
            let absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                folder.join(path)
            };
            let real = resolve(&absolute).unwrap_or(absolute);
            Place::Outside(
                named_with_folder(&real),
                clip(&real.display().to_string(), 400),
            )
        }
    }
}

fn with_exact(ask: Ask, exact: String) -> Ask {
    Ask {
        exact: Some(exact).filter(|e| !e.is_empty()),
        ..ask
    }
}

fn site(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    host.split(':').next().unwrap_or(host).to_owned()
}

fn quoted(text: &str) -> String {
    format!("“{}”", clip(text, 200))
}

fn magnesium_tool(tool: &str) -> Option<&'static str> {
    let name = ["mcp__plugin_magnesium_magnesium__", "mcp__magnesium__"]
        .iter()
        .find_map(|prefix| tool.strip_prefix(prefix))?;
    Some(match name {
        "context" | "memory" => "Reading your record",
        "learning_step" => "Saving your work",
        "search_sources" => "Searching your material",
        "sources" => "Reading your material",
        _ => "",
    })
    .filter(|words| !words.is_empty())
}

pub fn describe(tool: &str, input: &Value, request: &Value, folder: &Path) -> Ask {
    let field = |name: &str| input[name].as_str().map(str::trim).unwrap_or_default();
    let file = || field("file_path").to_owned() + field("notebook_path");
    match tool {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let path = file();
            let new = tool == "Write" && !path.is_empty() && !folder.join(&path).exists();
            match place(&path, folder) {
                Place::Plan(plan) => ask("change your plan", plan),
                Place::Course(name) if new => ask("add a file to this space", name),
                Place::Course(name) => ask("change a file in this space", name),
                Place::Outside(name, path) if new => {
                    with_exact(ask("add a file outside this space", name), path)
                }
                Place::Outside(name, path) => {
                    with_exact(ask("change a file outside this space", name), path)
                }
            }
        }
        "Read" => match place(&file(), folder) {
            Place::Plan(plan) => ask("read your plan", plan),
            Place::Course(name) => ask("read a file in this space", name),
            Place::Outside(name, path) => {
                with_exact(ask("read a file outside this space", name), path)
            }
        },
        "Grep" | "Glob" => {
            let pattern = quoted(field("pattern"));
            let looks = if tool == "Grep" {
                "search"
            } else {
                "look through"
            };
            match field("path") {
                "" => ask(&format!("{looks} your files"), pattern),
                path => match inside(Path::new(path), folder) {
                    Some(_) => ask(&format!("{looks} your files"), pattern),
                    None => with_exact(
                        ask(
                            &format!("{looks} files outside this space"),
                            format!("{pattern}, in {}", name_of(Path::new(path))),
                        ),
                        clip(path, 400),
                    ),
                },
            }
        }
        "WebFetch" => with_exact(
            ask("open a web page", site(field("url"))),
            clip(field("url"), 400),
        ),
        "WebSearch" => ask("search the web", quoted(field("query"))),
        "Bash" => Ask {
            exact: Some(clip(field("command"), 400)).filter(|c| !c.is_empty()),
            ..ask("run a command", clip(field("description"), 200))
        },
        other => match magnesium_tool(other) {
            Some(words) => ask("use one of Magnesium’s tools", words),
            None => {
                let name = request["display_name"]
                    .as_str()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or(other);
                let given = match input.as_object() {
                    Some(fields) if fields.is_empty() => String::new(),
                    None if input.is_null() => String::new(),
                    _ => format!(" {}", clip(&input.to_string(), 300)),
                };
                let said = request["description"]
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.contains('/') && !text.is_empty())
                    .map(|text| clip(text, 200))
                    .unwrap_or_default();
                Ask {
                    exact: Some(format!("{name}{given}")),
                    ..ask("use a tool this window doesn’t know", said)
                }
            }
        },
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    pub label: String,
    pub rules: Vec<Value>,
}

fn rule_words(rule: &Value, folder: &Path) -> Option<String> {
    let tool = rule["toolName"].as_str()?;
    let content = rule["ruleContent"].as_str().map(str::trim);
    let folder = |glob: &str| {
        let dir = rule_folder(glob, folder)?;
        let relative = inside(&dir, folder)?;
        Some(if relative.as_os_str().is_empty() {
            "this space".to_owned()
        } else {
            name_of(&relative)
        })
    };
    match (tool, content) {
        ("WebFetch", Some(content)) => content
            .strip_prefix("domain:")
            .map(|host| format!("pages from {host}")),
        ("WebSearch", None) => Some("web searches".into()),
        ("Bash", Some(command)) => Some(match command.strip_suffix(":*") {
            Some(start) => format!("{} commands", clip(start, 60)),
            None => "this command".into(),
        }),
        ("Read" | "Grep" | "Glob", Some(glob)) => {
            folder(glob).map(|dir| format!("reading files in {dir}"))
        }
        ("Edit" | "Write" | "MultiEdit", Some(glob)) => {
            folder(glob).map(|dir| format!("changes to files in {dir}"))
        }
        _ => None,
    }
}

pub fn grant(request: &Value, folder: &Path) -> Option<Grant> {
    if request["suppress_always_allow_rule"].as_bool() == Some(true) {
        return None;
    }
    let roots = roots(folder);
    let mut words: Vec<String> = Vec::new();
    let mut rules = Vec::new();
    let mut lasting = false;
    for suggestion in request["permission_suggestions"].as_array()? {
        let destination = suggestion["destination"].as_str();
        if !matches!(destination, Some("localSettings" | "session")) {
            continue;
        }
        let said: Option<Vec<String>> = match suggestion["type"].as_str() {
            Some("setMode") if suggestion["mode"] == "acceptEdits" => {
                Some(vec!["changes to files".into()])
            }
            Some("addRules") if suggestion["behavior"] == "allow" => suggestion["rules"]
                .as_array()
                .and_then(|list| list.iter().map(|rule| rule_words(rule, folder)).collect()),
            Some("addDirectories") => suggestion["directories"]
                .as_array()
                .filter(|dirs| {
                    dirs.iter().all(|dir| {
                        dir.as_str()
                            .is_some_and(|dir| roots.iter().any(|root| root == Path::new(dir)))
                    })
                })
                .map(|_| Vec::new()),
            _ => None,
        };
        let Some(said) = said else { continue };
        lasting |= destination == Some("localSettings");
        for each in said {
            if !words.contains(&each) {
                words.push(each);
            }
        }
        rules.push(suggestion.clone());
    }
    if words.is_empty() {
        return None;
    }
    let what = match words.as_slice() {
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        [] => unreachable!(),
    };
    let label = if lasting {
        format!("Always allow {what}")
    } else {
        format!("Allow {what} this session")
    };
    Some(Grant { label, rules })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn course() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".magnesium")).unwrap();
        std::fs::create_dir_all(dir.path().join("Study/midterm-1")).unwrap();
        std::fs::write(dir.path().join("Study/midterm-1/plan.md"), "# Midterm 1\n").unwrap();
        std::fs::write(dir.path().join("notes.md"), "notes\n").unwrap();
        dir
    }

    #[test]
    fn each_tool_a_lesson_has_is_said_in_the_learners_words() {
        let dir = course();
        let folder = dir.path();
        let at = |rest: &str| folder.join(rest).display().to_string();
        let said = |tool: &str, input: Value| {
            let ask = describe(tool, &input, &json!({"display_name": tool}), folder);
            (ask.title, ask.detail, ask.exact)
        };
        let words = |title: &str, detail: &str| (title.to_owned(), detail.to_owned(), None);

        assert_eq!(
            said(
                "Edit",
                json!({"file_path": at("Study/midterm-1/plan.md"), "old_string": "doing", "new_string": "done"})
            ),
            words("The tutor asks to change your plan", "Midterm 1")
        );
        assert_eq!(
            said(
                "Write",
                json!({"file_path": at("Study/midterm-1/graph.yaml"), "content": ""})
            ),
            words("The tutor asks to add a file to this space", "graph.yaml")
        );
        assert_eq!(
            said(
                "Write",
                json!({"file_path": at("Study/materials/summary.md"), "content": "hi"})
            ),
            words("The tutor asks to add a file to this space", "summary.md")
        );
        assert_eq!(
            said("Edit", json!({"file_path": at("notes.md")})),
            words("The tutor asks to change a file in this space", "notes.md")
        );
        let exact = |title: &str, detail: &str, exact: &str| {
            (title.to_owned(), detail.to_owned(), Some(exact.to_owned()))
        };
        assert_eq!(
            said(
                "Read",
                json!({"file_path": "/Users/someone/Downloads/other.txt"})
            ),
            exact(
                "The tutor asks to read a file outside this space",
                "other.txt, in Downloads",
                "/Users/someone/Downloads/other.txt"
            )
        );
        assert_eq!(
            said("Edit", json!({"file_path": "/Users/someone/.ssh/config"})),
            exact(
                "The tutor asks to change a file outside this space",
                "config, in .ssh",
                "/Users/someone/.ssh/config"
            )
        );
        assert_eq!(
            said(
                "Grep",
                json!({"pattern": "secret", "path": "/Users/someone/Downloads"})
            ),
            exact(
                "The tutor asks to search files outside this space",
                "“secret”, in Downloads",
                "/Users/someone/Downloads"
            )
        );
        assert_eq!(
            said(
                "WebFetch",
                json!({"url": "https://en.wikipedia.org/wiki/Michaelis%E2%80%93Menten_kinetics?q=my+answer", "prompt": "summary"})
            ),
            exact(
                "The tutor asks to open a web page",
                "en.wikipedia.org",
                "https://en.wikipedia.org/wiki/Michaelis%E2%80%93Menten_kinetics?q=my+answer"
            )
        );
        assert_eq!(
            said("WebSearch", json!({"query": "Lineweaver-Burk plot"})),
            words("The tutor asks to search the web", "“Lineweaver-Burk plot”")
        );
        assert_eq!(
            said(
                "Bash",
                json!({"command": "latexmk paper.tex", "description": "Build the paper"})
            ),
            (
                "The tutor asks to run a command".to_owned(),
                "Build the paper".to_owned(),
                Some("latexmk paper.tex".to_owned())
            )
        );
        for tool in crate::claude::TOOLS {
            if tool == "AskUserQuestion" {
                continue;
            }
            let ask = describe(tool, &json!({}), &json!({}), folder);
            assert!(!ask.title.contains("doesn’t know"), "{tool}: {ask:?}");
        }
    }

    #[test]
    fn a_file_reached_through_a_link_is_where_the_link_points() {
        let dir = course();
        let folder = dir.path();
        let away = tempfile::tempdir().unwrap();
        std::fs::write(away.path().join("secrets.txt"), "x").unwrap();
        std::fs::create_dir_all(away.path().join("elsewhere")).unwrap();
        std::fs::write(away.path().join("elsewhere/plan.md"), "x").unwrap();
        std::os::unix::fs::symlink(
            away.path().join("secrets.txt"),
            folder.join("notes-link.md"),
        )
        .unwrap();
        std::os::unix::fs::symlink(away.path().join("elsewhere"), folder.join("Study/linked"))
            .unwrap();
        let real = |rest: &str| {
            std::fs::canonicalize(away.path().join(rest))
                .unwrap()
                .display()
                .to_string()
        };
        let read = describe(
            "Read",
            &json!({"file_path": folder.join("notes-link.md").display().to_string()}),
            &json!({}),
            folder,
        );
        assert_eq!(
            read.title,
            "The tutor asks to read a file outside this space"
        );
        assert_eq!(
            read.detail,
            format!(
                "secrets.txt, in {}",
                name_of(&std::fs::canonicalize(away.path()).unwrap())
            )
        );
        assert_eq!(read.exact.as_deref(), Some(real("secrets.txt").as_str()));
        let plan = describe(
            "Edit",
            &json!({"file_path": folder.join("Study/linked/plan.md").display().to_string()}),
            &json!({}),
            folder,
        );
        assert_eq!(
            plan.title,
            "The tutor asks to change a file outside this space"
        );
        let new = describe(
            "Write",
            &json!({"file_path": folder.join("Study/linked/new.md").display().to_string()}),
            &json!({}),
            folder,
        );
        assert_eq!(new.title, "The tutor asks to add a file outside this space");
        let inside = describe(
            "Edit",
            &json!({"file_path": folder.join("notes.md").display().to_string()}),
            &json!({}),
            folder,
        );
        assert_eq!(
            (inside.title.as_str(), inside.exact),
            ("The tutor asks to change a file in this space", None)
        );
    }

    #[test]
    fn a_tool_the_window_does_not_know_shows_what_it_would_be_given() {
        let dir = course();
        let ask = describe(
            "mcp__calendar__create_event",
            &json!({"title": "Exam"}),
            &json!({"display_name": "create_event", "description": "Adds an event to your calendar"}),
            dir.path(),
        );
        assert_eq!(
            ask.title,
            "The tutor asks to use a tool this window doesn’t know"
        );
        assert_eq!(ask.detail, "Adds an event to your calendar");
        assert_eq!(
            ask.exact.as_deref(),
            Some(r#"create_event {"title":"Exam"}"#)
        );
        let own = describe(
            "mcp__plugin_magnesium_magnesium__learning_step",
            &json!({}),
            &json!({}),
            dir.path(),
        );
        assert_eq!(
            (own.title.as_str(), own.detail.as_str(), own.exact),
            (
                "The tutor asks to use one of Magnesium’s tools",
                "Saving your work",
                None
            )
        );
        let other = describe(
            "mcp__not_magnesium__learning_step",
            &json!({}),
            &json!({}),
            dir.path(),
        );
        assert_eq!(
            other.title,
            "The tutor asks to use a tool this window doesn’t know"
        );
        assert_eq!(
            other.exact.as_deref(),
            Some("mcp__not_magnesium__learning_step")
        );
    }

    #[test]
    fn always_says_what_it_would_allow() {
        let dir = course();
        let folder = dir.path();
        let label = |suggestions: Value| {
            grant(&json!({"permission_suggestions": suggestions}), folder).map(|g| g.label)
        };
        assert_eq!(
            label(json!([{"type": "setMode", "mode": "acceptEdits", "destination": "session"}]))
                .as_deref(),
            Some("Allow changes to files this session")
        );
        assert_eq!(
            label(
                json!([{"type": "addRules", "destination": "localSettings", "behavior": "allow",
                "rules": [{"toolName": "WebFetch", "ruleContent": "domain:en.wikipedia.org"}]}])
            )
            .as_deref(),
            Some("Always allow pages from en.wikipedia.org")
        );
        assert_eq!(
            label(
                json!([{"type": "addRules", "destination": "localSettings", "behavior": "allow",
                "rules": [{"toolName": "WebSearch"}]}])
            )
            .as_deref(),
            Some("Always allow web searches")
        );
        let real = std::fs::canonicalize(folder).unwrap();
        assert_eq!(
            label(json!([{"type": "addRules", "destination": "session", "behavior": "allow",
                "rules": [{"toolName": "Read", "ruleContent": format!("/{}/Study/**", real.display())}]}]))
            .as_deref(),
            Some("Allow reading files in Study this session")
        );
        assert_eq!(
            label(
                json!([{"type": "addRules", "destination": "session", "behavior": "allow",
                "rules": [{"toolName": "Edit", "ruleContent": format!("/{}/**", real.display())}]}])
            )
            .as_deref(),
            Some("Allow changes to files in this space this session")
        );
        assert_eq!(
            label(json!([
                {"type": "addRules", "destination": "localSettings", "behavior": "allow",
                 "rules": [{"toolName": "Bash", "ruleContent": "ls ~/Downloads > listing.txt"}]},
                {"type": "addDirectories", "destination": "session", "directories": [folder.display().to_string()]},
            ]))
            .as_deref(),
            Some("Always allow this command")
        );
        assert_eq!(
            label(
                json!([{"type": "addRules", "destination": "localSettings", "behavior": "allow",
                "rules": [{"toolName": "Bash", "ruleContent": "tectonic:*"}]}])
            )
            .as_deref(),
            Some("Always allow tectonic commands")
        );
    }

    #[test]
    fn always_is_not_offered_for_what_it_cannot_say_or_save_everywhere() {
        let dir = course();
        let folder = dir.path();
        let grant_of = |request: Value| grant(&request, folder);
        for suggestions in [
            json!([{"type": "addDirectories", "destination": "session", "directories": ["/Users/someone"]}]),
            json!([{"type": "addRules", "destination": "session", "behavior": "allow",
                "rules": [{"toolName": "Read", "ruleContent": "//Users/someone/Downloads/**"}]}]),
            json!([{"type": "addRules", "destination": "localSettings", "behavior": "allow",
                "rules": [{"toolName": "Edit", "ruleContent": "~/notes/**"}]}]),
            json!([{"type": "addRules", "destination": "session", "behavior": "allow",
                "rules": [{"toolName": "Read", "ruleContent": "//Users/someone/notes.md"}]}]),
            json!([{"type": "setMode", "mode": "bypassPermissions", "destination": "session"}]),
            json!([{"type": "addRules", "destination": "userSettings", "behavior": "allow", "rules": [{"toolName": "WebSearch"}]}]),
            json!([]),
        ] {
            assert_eq!(
                grant_of(json!({"permission_suggestions": suggestions})),
                None
            );
        }
        assert_eq!(grant_of(json!({})), None);
        assert_eq!(
            grant_of(
                json!({"suppress_always_allow_rule": true, "permission_suggestions": [
                {"type": "setMode", "mode": "acceptEdits", "destination": "session"}]})
            ),
            None
        );
        let both = grant_of(json!({"permission_suggestions": [
            {"type": "addRules", "destination": "localSettings", "behavior": "allow", "rules": [{"toolName": "WebSearch"}]},
            {"type": "addDirectories", "destination": "session", "directories": ["/Users/someone"]},
        ]}))
        .unwrap();
        assert_eq!(both.rules.len(), 1);
    }
}
