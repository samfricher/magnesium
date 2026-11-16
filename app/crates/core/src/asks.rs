//! A permission request from the agent, as the window puts it to the learner.
//!
//! Claude Code's request names a tool, what the tool was given, and what a
//! "don't ask again" would save. Its own words for these are the tool's name
//! and a path (Claude Code 2.1.322 sent `display_name: "Edit"` and
//! `description: "Study/midterm-1/plan.md"`), so the words here come from
//! what Magnesium knows about the request: the study folder and its plans,
//! which the runtime's digest lists, and the tools a lesson is given
//! (`claude::TOOLS`). A tool outside those is said to be one the window does
//! not know, and what it was given is shown as it is.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::claude::clip;

/// A permission request in the learner's words.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ask {
    /// What the tutor asks, as a sentence.
    pub title: String,
    /// What it would touch, in plain words, or empty.
    pub detail: String,
    /// The exact thing, when the plain words cannot show all of it.
    pub exact: Option<String>,
}

fn ask(title: &str, detail: impl Into<String>) -> Ask {
    Ask {
        title: format!("The tutor asks to {title}"),
        detail: detail.into(),
        exact: None,
    }
}

/// Where a path is, as the learner knows it.
enum Place {
    /// One of the folder's plans, by its name.
    Plan(String),
    /// A file in the study folder, by its name.
    Course(String),
    /// Somewhere else on the Mac: the file's name and its folder's.
    Outside(String),
}

/// The study root and the open folder, each as given and as the disk
/// resolves it (`/tmp` is `/private/tmp` on a Mac).
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

/// The path inside the study root, if it is in it.
fn inside(path: &Path, folder: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        folder.join(path)
    };
    // A file that does not exist yet is found by its folder.
    let real = path
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .zip(path.file_name())
        .map(|(parent, name)| parent.join(name));
    let root = crate::direct::study_root(folder);
    let real_root = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    let relative = [Some(&path), real.as_ref()]
        .into_iter()
        .flatten()
        .find_map(|each| {
            each.strip_prefix(&root)
                .or_else(|_| each.strip_prefix(&real_root))
                .ok()
                .map(Path::to_path_buf)
        });
    relative.filter(|relative| {
        !relative
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    })
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// "other.txt, in Downloads": a file's name and its folder's, never the path.
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

/// "Midterm 1" from "midterm-1", for a plan the digest does not name.
fn title_of_target(target: &str) -> String {
    let words = target.replace(['-', '_'], " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The plan a path in the study root belongs to: the digest's plan whose file
/// it is, or whose folder holds it (a plan's `graph.yaml`).
fn plan_of(relative: &Path, folder: &Path) -> Option<String> {
    let parts: Vec<String> = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    let [study, target, file] = parts.as_slice() else {
        return None;
    };
    if study != "Study" || !(file == "plan.md" || file == "graph.yaml") {
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
        None => Place::Outside(named_with_folder(path)),
    }
}

/// "en.wikipedia.org" from a web address.
fn site(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    host.split(':').next().unwrap_or(host).to_owned()
}

fn quoted(text: &str) -> String {
    format!("“{}”", clip(text, 200))
}

/// Magnesium's own tools, from its plugin, by what each does.
fn magnesium_tool(tool: &str) -> Option<&'static str> {
    if !(tool.starts_with("mcp__") && tool.contains(crate::claude::PLUGIN)) {
        return None;
    }
    let name = tool.rsplit("__").next().unwrap_or_default();
    Some(match name {
        "context" | "memory" => "Reading your record",
        "learning_step" => "Saving your work",
        "search_sources" => "Searching your course material",
        "sources" => "Reading your course material",
        _ => "",
    })
    .filter(|words| !words.is_empty())
}

/// A permission request as the learner reads it: what the tutor asks, what it
/// would touch, and the exact thing when the words cannot show all of it.
pub fn describe(tool: &str, input: &Value, request: &Value, folder: &Path) -> Ask {
    let field = |name: &str| input[name].as_str().map(str::trim).unwrap_or_default();
    let file = || field("file_path").to_owned() + field("notebook_path");
    match tool {
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let path = file();
            let new = tool == "Write" && !path.is_empty() && !folder.join(&path).exists();
            match place(&path, folder) {
                Place::Plan(plan) => ask("change your plan", plan),
                Place::Course(name) if new => ask("add a file to this course", name),
                Place::Course(name) => ask("change a file in this course", name),
                Place::Outside(name) if new => ask("add a file outside this course", name),
                Place::Outside(name) => ask("change a file outside this course", name),
            }
        }
        "Read" => match place(&file(), folder) {
            Place::Plan(plan) => ask("read your plan", plan),
            Place::Course(name) => ask("read a file in this course", name),
            Place::Outside(name) => ask("read a file outside this course", name),
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
                    None => ask(
                        &format!("{looks} files outside this course"),
                        format!("{pattern}, in {}", name_of(Path::new(path))),
                    ),
                },
            }
        }
        "WebFetch" => ask("open a web page", site(field("url"))),
        "WebSearch" => ask("search the web", quoted(field("query"))),
        // The command is shown whole: the tutor's own words for it are what it
        // says it does, and the learner allows what it does.
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
                // Claude Code's own subtitle, when it is words and not a path.
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

/// What "always" would allow, and the agent's own rules for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Grant {
    /// The choice's label: "Always allow pages from en.wikipedia.org".
    pub label: String,
    /// The agent's suggestions the window echoes back when it is chosen.
    pub rules: Vec<Value>,
}

/// One rule of the agent's, as the thing it allows: "pages from
/// en.wikipedia.org". None for a rule the window cannot put in words, which
/// is then never echoed back.
fn rule_words(rule: &Value) -> Option<String> {
    let tool = rule["toolName"].as_str()?;
    let content = rule["ruleContent"].as_str().map(str::trim);
    let folder = |glob: &str| {
        let dir = glob
            .trim_end_matches("/**")
            .trim_end_matches("/*")
            .trim_start_matches('/');
        name_of(Path::new(&format!("/{dir}")))
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
        ("Read" | "Grep" | "Glob", Some(glob)) if glob.ends_with("/**") => {
            Some(format!("reading files in {}", folder(glob)))
        }
        ("Edit" | "Write" | "MultiEdit", Some(glob)) if glob.ends_with("/**") => {
            Some(format!("changes to files in {}", folder(glob)))
        }
        _ => None,
    }
}

/// What the agent suggests saving for "don't ask again", as one choice: only
/// suggestions for this folder (`localSettings`) or this sitting (`session`)
/// that the window can put in words. None when there is none, or when the
/// agent says no lasting rule fits this request.
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
            // Claude Code's mode for accepting edits, for the rest of the sitting.
            Some("setMode") if suggestion["mode"] == "acceptEdits" => {
                Some(vec!["changes to files".into()])
            }
            Some("addRules") if suggestion["behavior"] == "allow" => suggestion["rules"]
                .as_array()
                .and_then(|list| list.iter().map(rule_words).collect()),
            // A folder the agent may reach: said only when it is the study
            // folder, which the agent works in already.
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

    /// A study folder with one plan, as `magnesium init` and a plan leave it.
    fn course() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".magnesium")).unwrap();
        std::fs::create_dir_all(dir.path().join("Study/midterm-1")).unwrap();
        std::fs::write(dir.path().join("Study/midterm-1/plan.md"), "# Midterm 1\n").unwrap();
        std::fs::write(dir.path().join("notes.md"), "notes\n").unwrap();
        dir
    }

    /// The requests Claude Code 2.1.322 sent on 2026-11-15 for the tools a
    /// lesson has, with the paths moved into a made-up folder.
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

        // No digest without the program: the plan is named from its folder.
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
            words("The tutor asks to change your plan", "Midterm 1")
        );
        assert_eq!(
            said(
                "Write",
                json!({"file_path": at("Study/materials/summary.md"), "content": "hi"})
            ),
            words("The tutor asks to add a file to this course", "summary.md")
        );
        assert_eq!(
            said("Edit", json!({"file_path": at("notes.md")})),
            words("The tutor asks to change a file in this course", "notes.md")
        );
        assert_eq!(
            said(
                "Read",
                json!({"file_path": "/Users/someone/Downloads/other.txt"})
            ),
            words(
                "The tutor asks to read a file outside this course",
                "other.txt, in Downloads"
            )
        );
        assert_eq!(
            said(
                "Grep",
                json!({"pattern": "secret", "path": "/Users/someone/Downloads"})
            ),
            words(
                "The tutor asks to search files outside this course",
                "“secret”, in Downloads"
            )
        );
        assert_eq!(
            said(
                "WebFetch",
                json!({"url": "https://en.wikipedia.org/wiki/Michaelis%E2%80%93Menten_kinetics", "prompt": "summary"})
            ),
            words("The tutor asks to open a web page", "en.wikipedia.org")
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
        // Every tool a lesson is given has words of its own: a tool added to
        // the lesson without them would be called one the window does not know.
        for tool in crate::claude::TOOLS {
            if tool == "AskUserQuestion" {
                continue;
            }
            let ask = describe(tool, &json!({}), &json!({}), folder);
            assert!(!ask.title.contains("doesn’t know"), "{tool}: {ask:?}");
        }
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
        // Magnesium's own tools are said by what they do.
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
    }

    /// The suggestions Claude Code 2.1.322 sent with each request on 2026-11-15.
    #[test]
    fn always_says_what_it_would_allow() {
        let dir = course();
        let folder = dir.path();
        let label = |suggestions: Value| {
            grant(&json!({"permission_suggestions": suggestions}), folder).map(|g| g.label)
        };
        // An edit's "don't ask again" is Claude Code's mode for accepting
        // every edit, for the rest of the sitting.
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
        assert_eq!(
            label(
                json!([{"type": "addRules", "destination": "session", "behavior": "allow",
                "rules": [{"toolName": "Read", "ruleContent": "//Users/someone/Downloads/**"}]}])
            )
            .as_deref(),
            Some("Allow reading files in Downloads this session")
        );
        // A command, and the study folder the agent works in already.
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
        // A folder outside the course, a mode the window has no words for, a
        // rule for every folder, and nothing at all.
        for suggestions in [
            json!([{"type": "addDirectories", "destination": "session", "directories": ["/Users/someone"]}]),
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
        // The agent says no lasting rule fits this request.
        assert_eq!(
            grant_of(
                json!({"suppress_always_allow_rule": true, "permission_suggestions": [
                {"type": "setMode", "mode": "acceptEdits", "destination": "session"}]})
            ),
            None
        );
        // What it cannot say is not echoed back with what it can.
        let both = grant_of(json!({"permission_suggestions": [
            {"type": "addRules", "destination": "localSettings", "behavior": "allow", "rules": [{"toolName": "WebSearch"}]},
            {"type": "addDirectories", "destination": "session", "directories": ["/Users/someone"]},
        ]}))
        .unwrap();
        assert_eq!(both.rules.len(), 1);
    }
}
