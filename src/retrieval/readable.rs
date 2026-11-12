//! `magnesium searchable`: whether the tutor's source search will read each
//! file named, and why not when it will not.
//!
//! Read-only. It writes nothing, not even the search index, and it asks the
//! rules that search itself uses: the exclusion policy in `retrieval.rs`,
//! the kinds of file it reads, its size limits, the content check for
//! passwords and keys, and the PDF text extractor. It changes none of them.
//! A study window uses it after the learner adds material, so it can say in
//! plain words what the tutor will not find.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;

use serde_json::{Value, json};

use super::manifest::convert::worker_config_from_env;
use super::manifest::pdf::extract_pdf_from_file;
use super::{
    ExclusionReason, MAX_PDF_SOURCE_BYTES, MAX_SOURCE_BYTES, SensitivityClass, SourceKind,
    credential_word_in_name, sensitive_content, source_exclusion, source_kind,
};
use crate::workspace::Workspace;

/// What search does with one file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Verdict {
    /// The path as the study folder sees it, or as it was given when it is outside.
    pub(crate) path: String,
    /// Whether search will find text in this file.
    pub(crate) searchable: bool,
    /// A short word for why not, or for a caveat on a file that is searchable.
    pub(crate) reason: Option<&'static str>,
    /// The word in the file's name that left it out, for `name_word`.
    pub(crate) word: Option<&'static str>,
    /// One plain sentence, the same facts as `reason`.
    pub(crate) detail: String,
    /// For a PDF: how many pages it has, and how many of them have no text.
    pub(crate) pages: Option<usize>,
    pub(crate) pages_without_text: Option<usize>,
}

impl Verdict {
    fn yes(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            searchable: true,
            reason: None,
            word: None,
            detail: "Search reads it.".to_owned(),
            pages: None,
            pages_without_text: None,
        }
    }

    fn no(path: &str, reason: &'static str, detail: &str) -> Self {
        Self {
            searchable: false,
            reason: Some(reason),
            detail: detail.to_owned(),
            ..Self::yes(path)
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "path": self.path,
            "searchable": self.searchable,
            "reason": self.reason,
            "word": self.word,
            "detail": self.detail,
            "pages": self.pages,
            "pages_without_text": self.pages_without_text,
        })
    }
}

/// What the PDF text extractor found in one file.
pub(crate) type PdfPages = Result<Vec<String>, String>;

/// The verdict for one file of a study folder. `relative` is its path from
/// the folder's root. `pdf_pages` reads a PDF's text, one string for each
/// page; a test gives it a stand-in.
pub(crate) fn check(
    root: &Path,
    relative: &Path,
    pdf_pages: impl FnOnce(fs::File) -> PdfPages,
) -> Verdict {
    let shown = relative.to_string_lossy().into_owned();
    let exclusion = match source_exclusion(root, relative) {
        Ok(exclusion) => exclusion,
        Err(error) => {
            return Verdict::no(
                &shown,
                "unreadable",
                &format!("Its place in the study folder could not be checked: {error}."),
            );
        }
    };
    if let Some(reason) = exclusion {
        return left_out(&shown, relative, &reason);
    }
    let full = root.join(relative);
    let metadata = match fs::symlink_metadata(&full) {
        Ok(metadata) if metadata.is_file() => metadata,
        _ => {
            return Verdict::no(
                &shown,
                "missing",
                "There is no such file in the study folder.",
            );
        }
    };
    let Some(kind) = source_kind(relative) else {
        let reason = if metadata.len() > MAX_SOURCE_BYTES as u64 {
            "too_large"
        } else {
            "unsupported_type"
        };
        return Verdict::no(
            &shown,
            reason,
            if reason == "too_large" {
                "It is too large for search."
            } else {
                "Search does not read this kind of file. It reads PDFs, text, Markdown, web pages and code."
            },
        );
    };
    match kind {
        SourceKind::Text => check_text(&shown, &full, metadata.len()),
        SourceKind::Pdf => check_pdf(&shown, &full, metadata.len(), pdf_pages),
    }
}

fn left_out(shown: &str, relative: &Path, reason: &ExclusionReason) -> Verdict {
    match reason {
        ExclusionReason::Sensitive(SensitivityClass::Credential)
            if credential_word_in_name(relative).is_some() =>
        {
            let word = credential_word_in_name(relative).unwrap_or_default();
            let why = if matches!(word, "key" | "keys") {
                "in case it is an answer key or holds a password"
            } else {
                "in case it holds a password"
            };
            Verdict {
                word: Some(word),
                ..Verdict::no(
                    shown,
                    "name_word",
                    &format!("Search leaves out a file whose name has the word \"{word}\", {why}."),
                )
            }
        }
        ExclusionReason::Sensitive(_) => Verdict::no(
            shown,
            "private_name",
            "Search leaves it out, because its name or its folder's name looks like a password or key file.",
        ),
        ExclusionReason::Magnesiumignore => Verdict::no(
            shown,
            "ignored",
            "The folder's ignore list, .magnesium/magnesiumignore, leaves it out of search.",
        ),
        ExclusionReason::BuiltIn => Verdict::no(
            shown,
            "built_in_folder",
            "It sits in a folder that search never reads, such as node_modules or build.",
        ),
        ExclusionReason::NestedRepository => Verdict::no(
            shown,
            "other_study_folder",
            "It sits in a folder that is a project or study folder of its own.",
        ),
        ExclusionReason::PrivateExport => Verdict::no(
            shown,
            "private_export",
            "It sits in an exported copy of a learner record, which search never reads.",
        ),
        ExclusionReason::Symlink => Verdict::no(
            shown,
            "link",
            "It is a link to a file, not the file itself, and search does not follow links.",
        ),
        ExclusionReason::EscapesWorkspace => {
            Verdict::no(shown, "outside", "It is outside the study folder.")
        }
        ExclusionReason::TooLarge { .. } => {
            Verdict::no(shown, "too_large", "It is too large for search.")
        }
        ExclusionReason::Unsupported => Verdict::no(
            shown,
            "unsupported_type",
            "Search does not read this kind of file.",
        ),
        ExclusionReason::Unreadable(_) => {
            Verdict::no(shown, "unreadable", "The file could not be read.")
        }
    }
}

fn read_all(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn check_text(shown: &str, full: &Path, size: u64) -> Verdict {
    if size > MAX_SOURCE_BYTES as u64 {
        return Verdict::no(shown, "too_large", "It is too large for search.");
    }
    match read_all(full, MAX_SOURCE_BYTES as u64) {
        Ok(bytes) if sensitive_content(&bytes).is_some() => Verdict::no(
            shown,
            "secret_content",
            "Search leaves it out, because its text looks like it holds a password or key.",
        ),
        Ok(_) => Verdict::yes(shown),
        Err(_) => Verdict::no(shown, "unreadable", "The file could not be read."),
    }
}

fn check_pdf(
    shown: &str,
    full: &Path,
    size: u64,
    pdf_pages: impl FnOnce(fs::File) -> PdfPages,
) -> Verdict {
    if size > MAX_PDF_SOURCE_BYTES as u64 {
        return Verdict::no(shown, "too_large", "It is too large for search.");
    }
    let Ok(file) = fs::File::open(full) else {
        return Verdict::no(shown, "unreadable", "The file could not be read.");
    };
    let pages = match pdf_pages(file) {
        Ok(pages) => pages,
        Err(_) => {
            return Verdict::no(
                shown,
                "unreadable",
                "Its text could not be read, so it cannot be searched.",
            );
        }
    };
    let without_text = pages.iter().filter(|page| page.trim().is_empty()).count();
    let counts = |verdict: Verdict| Verdict {
        pages: Some(pages.len()),
        pages_without_text: Some(without_text),
        ..verdict
    };
    if pages
        .iter()
        .any(|page| sensitive_content(page.as_bytes()).is_some())
    {
        return counts(Verdict::no(
            shown,
            "secret_content",
            "Search leaves it out, because its text looks like it holds a password or key.",
        ));
    }
    if without_text == pages.len() {
        // The same rule as the indexer's: a PDF whose pages are all blank
        // gets text only from the optional conversion worker, and nothing
        // turns that on by itself.
        return counts(if worker_config_from_env().is_some() {
            Verdict {
                reason: Some("scan_converted"),
                detail: "It is a scan with no text. The conversion worker will try to read it."
                    .into(),
                ..Verdict::yes(shown)
            }
        } else {
            Verdict::no(
                shown,
                "scanned",
                "It is a scan with no text, so its text cannot be searched.",
            )
        });
    }
    counts(if without_text > 0 {
        Verdict {
            reason: Some("partly_scanned"),
            detail: format!(
                "{without_text} of its {} pages have no text, so those pages cannot be searched.",
                pages.len()
            ),
            ..Verdict::yes(shown)
        }
    } else {
        Verdict::yes(shown)
    })
}

/// The text extractor search uses, run on one file.
fn extract(file: fs::File) -> PdfPages {
    extract_pdf_from_file(file, None).map_err(|error| error.to_string())
}

/// `path` as a path from the study folder's root, or `None` when it lies
/// outside the folder. Nothing is resolved through the disk, so a link keeps
/// its own name and search's rule about links applies to it.
fn from_root(root: &Path, cwd: &Path, given: &Path) -> Option<PathBuf> {
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        cwd.join(given)
    };
    let mut parts: Vec<Component> = Vec::new();
    for part in joined.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(parts.last(), Some(Component::Normal(_))) {
                    return None;
                }
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let normal: PathBuf = parts.into_iter().collect();
    let canonical_root = fs::canonicalize(root).ok();
    [Some(root), canonical_root.as_deref()]
        .into_iter()
        .flatten()
        .find_map(|base| normal.strip_prefix(base).ok())
        .map(Path::to_path_buf)
        .filter(|relative| !relative.as_os_str().is_empty())
}

fn verdicts(root: &Path, cwd: &Path, given: &[PathBuf]) -> Vec<Verdict> {
    given
        .iter()
        .map(|path| match from_root(root, cwd, path) {
            Some(relative) => check(root, &relative, extract),
            None => Verdict::no(
                &path.to_string_lossy(),
                "outside",
                "It is outside the study folder.",
            ),
        })
        .collect()
}

fn render(verdicts: &[Verdict]) -> String {
    verdicts
        .iter()
        .map(|verdict| {
            let state = if verdict.searchable {
                "search reads it"
            } else {
                "left out of search"
            };
            let note = match (verdict.searchable, verdict.reason) {
                (true, None) => String::new(),
                _ => format!(". {}", verdict.detail),
            };
            format!("{}: {state}{note}\n", verdict.path)
        })
        .collect()
}

/// `magnesium searchable [--json] PATH...`, run in a study folder. In a folder
/// Magnesium has not been set up in, the folder itself is the root: the rules
/// are the ones it will have once it is set up, with no ignore list yet.
pub(crate) fn run(paths: &[PathBuf], as_json: bool) -> ExitCode {
    let result = (|| -> Result<Vec<Verdict>, String> {
        let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
        let root = match Workspace::discover(&cwd).map_err(|error| error.to_string())? {
            Some(workspace) => workspace.root().to_path_buf(),
            None => cwd.clone(),
        };
        Ok(verdicts(&root, &cwd, paths))
    })();
    match result {
        Ok(verdicts) => {
            if as_json {
                let files: Vec<Value> = verdicts.iter().map(Verdict::to_json).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({ "files": files })).unwrap_or_default()
                );
            } else {
                print!("{}", render(&verdicts));
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("magnesium: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> tempfile_dir::Dir {
        tempfile_dir::Dir::new()
    }

    /// A scratch study folder under the system's temp directory, removed on drop.
    mod tempfile_dir {
        use std::path::{Path, PathBuf};

        pub(super) struct Dir(PathBuf);

        impl Dir {
            pub(super) fn new() -> Self {
                let path =
                    std::env::temp_dir().join(crate::learner::store::new_id("searchable").unwrap());
                std::fs::create_dir_all(&path).unwrap();
                Self(path)
            }

            pub(super) fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    fn put(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn never(_: fs::File) -> PdfPages {
        panic!("a text file is not given to the PDF reader");
    }

    fn pages(texts: &[&str]) -> impl FnOnce(fs::File) -> PdfPages {
        let texts: Vec<String> = texts.iter().map(|t| (*t).to_owned()).collect();
        move |_| Ok(texts)
    }

    #[test]
    fn a_text_file_in_the_folder_is_read() {
        let dir = folder();
        put(dir.path(), "Study/stats/notes.txt", "Bayes' rule");
        let verdict = check(dir.path(), Path::new("Study/stats/notes.txt"), never);
        assert!(verdict.searchable, "{verdict:?}");
        assert_eq!(verdict.reason, None);
    }

    #[test]
    fn a_word_in_the_name_is_named() {
        let dir = folder();
        for (name, word) in [
            ("Study/stats/api-key notes.txt", "key"),
            ("Study/stats/Week 4 Tokens.md", "tokens"),
            ("Study/stats/secret.txt", "secret"),
            ("Study/stats/my_credentials.md", "credentials"),
        ] {
            put(dir.path(), name, "text");
            let verdict = check(dir.path(), Path::new(name), never);
            assert!(!verdict.searchable, "{name}");
            assert_eq!(verdict.reason, Some("name_word"), "{name}");
            assert_eq!(verdict.word, Some(word), "{name}");
            assert!(verdict.detail.contains(word), "{name}: {}", verdict.detail);
        }
        // A word that only contains one of those is not the word.
        put(dir.path(), "Study/stats/monkey-turkey.txt", "text");
        assert!(
            check(
                dir.path(),
                Path::new("Study/stats/monkey-turkey.txt"),
                never
            )
            .searchable
        );
    }

    #[test]
    fn a_course_document_is_never_a_credential_by_its_name_alone() {
        let dir = folder();
        for name in [
            "Study/Token economies.pdf",
            "Study/The secret life of bees.pdf",
            "Study/Teaching credentials.PDF",
        ] {
            put(dir.path(), name, "%PDF");
            let verdict = check(dir.path(), Path::new(name), pages(&["Bees"]));
            assert!(verdict.searchable, "{name}: {verdict:?}");
            assert!(
                !crate::retrieval::source_relative_path_is_excluded(dir.path(), Path::new(name))
                    .unwrap(),
                "{name}"
            );
        }
        // Its text is still checked.
        let pdf = check(
            dir.path(),
            Path::new("Study/The secret life of bees.pdf"),
            pages(&["password = hunter2"]),
        );
        assert_eq!(pdf.reason, Some("secret_content"));
        // A file a person types as text is still left out by the word: a
        // pasted secret would land there.
        for (name, word) in [
            ("Study/api tokens.md", "tokens"),
            ("Study/secret.html", "secret"),
            ("Study/notes on secrets.txt", "secrets"),
            ("Study/tokens.json", "tokens"),
        ] {
            put(dir.path(), name, "text");
            let verdict = check(dir.path(), Path::new(name), never);
            assert_eq!(
                (verdict.reason, verdict.word),
                (Some("name_word"), Some(word)),
                "{name}"
            );
        }
        // "key" in a document's name still leaves it out: it may be an
        // answer key.
        for name in [
            "Study/Midterm answer key.pdf",
            "Study/Secret answer key.pdf",
        ] {
            put(dir.path(), name, "%PDF");
            let key = check(dir.path(), Path::new(name), never);
            assert_eq!(
                (key.reason, key.word),
                (Some("name_word"), Some("key")),
                "{name}"
            );
        }
    }

    #[test]
    fn a_private_key_file_and_an_env_file_are_left_out_by_their_names() {
        let dir = folder();
        for name in ["Study/id_rsa", "Study/server.pem", "Study/.env"] {
            put(dir.path(), name, "x");
            let verdict = check(dir.path(), Path::new(name), never);
            assert_eq!(verdict.reason, Some("private_name"), "{name}");
        }
    }

    #[test]
    fn the_folders_ignore_list_and_the_built_in_folders_leave_a_file_out() {
        let dir = folder();
        put(dir.path(), ".magnesium/magnesiumignore", "drafts/\n");
        put(dir.path(), "Study/drafts/a.md", "x");
        put(dir.path(), "Study/node_modules/a.md", "x");
        assert_eq!(
            check(dir.path(), Path::new("Study/drafts/a.md"), never).reason,
            Some("ignored")
        );
        assert_eq!(
            check(dir.path(), Path::new("Study/node_modules/a.md"), never).reason,
            Some("built_in_folder")
        );
    }

    #[test]
    fn the_same_answer_as_the_policy_search_uses() {
        let dir = folder();
        put(dir.path(), ".magnesium/magnesiumignore", "*.log\n");
        for name in [
            "a.md",
            "keys.md",
            "x.log",
            "Study/ok.txt",
            ".env",
            "notes.docx",
        ] {
            put(dir.path(), name, "text");
            let excluded =
                crate::retrieval::source_relative_path_is_excluded(dir.path(), Path::new(name))
                    .unwrap();
            let verdict = check(dir.path(), Path::new(name), never);
            if excluded {
                assert!(!verdict.searchable, "{name}");
            }
            if verdict.searchable {
                assert!(!excluded, "{name}");
            }
        }
    }

    #[test]
    fn an_unsupported_kind_a_missing_file_and_a_link_are_not_read() {
        let dir = folder();
        put(dir.path(), "Study/slides.pptx", "x");
        assert_eq!(
            check(dir.path(), Path::new("Study/slides.pptx"), never).reason,
            Some("unsupported_type")
        );
        assert_eq!(
            check(dir.path(), Path::new("Study/nothing.md"), never).reason,
            Some("missing")
        );
        #[cfg(unix)]
        {
            put(dir.path(), "Study/real.md", "x");
            std::os::unix::fs::symlink(
                dir.path().join("Study/real.md"),
                dir.path().join("Study/alias.md"),
            )
            .unwrap();
            assert_eq!(
                check(dir.path(), Path::new("Study/alias.md"), never).reason,
                Some("link")
            );
        }
    }

    #[test]
    fn text_that_holds_a_password_is_left_out() {
        let dir = folder();
        put(dir.path(), "Study/setup.md", "password = hunter2\n");
        assert_eq!(
            check(dir.path(), Path::new("Study/setup.md"), never).reason,
            Some("secret_content")
        );
    }

    #[test]
    fn a_pdf_with_text_is_read_and_a_scan_is_not() {
        let dir = folder();
        put(dir.path(), "Study/week4.pdf", "%PDF");
        let relative = Path::new("Study/week4.pdf");
        let readable = check(dir.path(), relative, pages(&["Bayes", "Priors"]));
        assert!(readable.searchable);
        assert_eq!(readable.reason, None);
        assert_eq!(
            (readable.pages, readable.pages_without_text),
            (Some(2), Some(0))
        );

        let scan = check(dir.path(), relative, pages(&["", "  \n"]));
        assert!(!scan.searchable);
        assert_eq!(scan.reason, Some("scanned"));
        assert!(scan.detail.contains("cannot be searched"));
        assert_eq!((scan.pages, scan.pages_without_text), (Some(2), Some(2)));

        let part = check(dir.path(), relative, pages(&["Bayes", ""]));
        assert!(part.searchable);
        assert_eq!(part.reason, Some("partly_scanned"));
        assert_eq!(part.pages_without_text, Some(1));

        let broken = check(dir.path(), relative, |_| Err("not a PDF".to_owned()));
        assert_eq!(broken.reason, Some("unreadable"));
    }

    #[test]
    fn a_pdf_whose_name_has_a_word_is_never_given_to_the_reader() {
        let dir = folder();
        put(dir.path(), "Study/answer-key.pdf", "%PDF");
        let verdict = check(dir.path(), Path::new("Study/answer-key.pdf"), never);
        assert_eq!(verdict.reason, Some("name_word"));
    }

    #[test]
    fn paths_are_taken_from_the_study_folder_and_never_from_outside_it() {
        let dir = folder();
        put(dir.path(), "Study/a.md", "x");
        let root = dir.path();
        let cwd = root.join("Study");
        assert_eq!(
            from_root(root, &cwd, Path::new("a.md")),
            Some(PathBuf::from("Study/a.md"))
        );
        assert_eq!(
            from_root(root, root, Path::new("./Study/../Study/a.md")),
            Some(PathBuf::from("Study/a.md"))
        );
        assert_eq!(
            from_root(root, root, &root.join("Study/a.md")),
            Some(PathBuf::from("Study/a.md"))
        );
        assert_eq!(from_root(root, root, Path::new("../elsewhere.md")), None);
        assert_eq!(from_root(root, root, Path::new("/etc/hosts")), None);
        assert_eq!(from_root(root, root, Path::new("Study/../..")), None);
        let outside = verdicts(root, root, &[PathBuf::from("../elsewhere.md")]);
        assert_eq!(outside[0].reason, Some("outside"));
    }

    #[test]
    fn the_text_form_says_what_search_reads_and_what_it_leaves_out() {
        let text = render(&[
            Verdict::yes("a.md"),
            Verdict::no(
                "b.pdf",
                "scanned",
                "It is a scan with no text, so its text cannot be searched.",
            ),
        ]);
        assert_eq!(
            text,
            "a.md: search reads it\nb.pdf: left out of search. It is a scan with no text, so its text cannot be searched.\n"
        );
    }
}
