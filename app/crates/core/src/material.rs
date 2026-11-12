//! Course material the learner adds from the window: slides, notes, a pasted
//! page. The window copies it into the study folder and says, in plain
//! lines, what it added and what the tutor's search cannot read.
//!
//! The window decides none of what search reads. It asks the runtime
//! (`magnesium searchable`), which runs search's own rules, and words the
//! answer. A file that looks like an answer key is added like any other and
//! never named or opened here: the window's rule for keys is
//! [`looks_like_a_key`].

use std::fs;
use std::path::{Component, Path, PathBuf};

use chrono::Local;
use serde::Serialize;
use serde_json::Value;

use crate::study::looks_like_a_key;

/// Where added material goes, under the study root.
///
/// Under `Study/` because that is where the tutor already keeps everything
/// of the course (its plans, papers and the learner's handed-in work), so
/// the material sits with the rest and a learner can find it in Finder. Not
/// in `.magnesium/`, which search never reads. Not in a folder search skips
/// by name (`build`, `dist`, `node_modules`, and so on), and the name
/// `materials` is not one of those. A learner's own ignore list can still
/// leave a file out, and `magnesium searchable` says so.
pub const MATERIALS: &str = "Study/materials";

/// The most one file may weigh, and the most files one folder may bring.
const MAX_BYTES: u64 = 200 * 1024 * 1024;
const MAX_FILES: usize = 300;
/// Pasted text is limited to what search reads of one text file.
const MAX_PASTED_BYTES: usize = 8 * 1024 * 1024;
/// How many things search cannot read are listed before "and N more".
const LINES_BEFORE_MORE: usize = 4;

/// One file the window saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    /// From the study root.
    pub relative: String,
    /// The name the learner knows it by.
    pub name: String,
    /// Whether it looks like an answer key. It is saved, and never named.
    pub key: bool,
}

/// What an add did, as the page shows it.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Added {
    /// Paths from the study root of the files saved, answer keys left out.
    pub saved: Vec<String>,
    /// How many saved files look like answer keys.
    pub keys: usize,
    /// What the window says, one plain line each.
    pub lines: Vec<String>,
}

/// A name that is safe to write: no path separators, no control characters,
/// not hidden, not endless.
fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == ':' || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        return "file".to_owned();
    }
    if cleaned.chars().count() <= 120 {
        return cleaned;
    }
    // Long names keep their ending, so the kind of file still shows.
    let (stem, ext) = split_name(&cleaned);
    let keep = 120usize.saturating_sub(ext.chars().count() + 1).max(1);
    let stem: String = stem.chars().take(keep).collect();
    if ext.is_empty() {
        stem
    } else {
        format!("{stem}.{ext}")
    }
}

fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(at) if at > 0 && at + 1 < name.len() => (&name[..at], &name[at + 1..]),
        _ => (name, ""),
    }
}

/// A name in `dir` that is free: `name.ext`, then `name 2.ext`, and so on.
fn free_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() && dir.join(name).symlink_metadata().is_err() {
        return name.to_owned();
    }
    let (stem, ext) = split_name(name);
    (2..)
        .map(|n| {
            if ext.is_empty() {
                format!("{stem} {n}")
            } else {
                format!("{stem} {n}.{ext}")
            }
        })
        .find(|candidate| dir.join(candidate).symlink_metadata().is_err())
        .unwrap_or_else(|| name.to_owned())
}

/// Make `relative` under `root`, refusing to go through a link. A link in
/// the way could lead out of the study folder.
fn make_dir(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return Err("That place is not inside the study folder.".into());
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(format!(
                    "{} is a link, so nothing is added through it.",
                    path.strip_prefix(root).unwrap_or(&path).display()
                ));
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(format!(
                    "{} is a file, not a folder, so material cannot go in it.",
                    path.strip_prefix(root).unwrap_or(&path).display()
                ));
            }
            Ok(_) => {}
            Err(_) => fs::create_dir(&path)
                .map_err(|e| format!("The study folder could not be written to: {e}."))?,
        }
    }
    Ok(path)
}

fn relative_of(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .into_owned()
}

/// Copy one file into `dir`. A file named like another keeps both.
fn copy_file(root: &Path, dir: &Path, source: &Path, name: &str) -> Result<Saved, String> {
    let meta = fs::metadata(source).map_err(|_| format!("\"{name}\" could not be read."))?;
    if !meta.is_file() {
        return Err(format!("\"{name}\" is not a file."));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!(
            "\"{name}\" is larger than 200 MB, so it was not added."
        ));
    }
    let target_name = free_name(dir, &clean_name(name));
    let target = dir.join(&target_name);
    fs::copy(source, &target).map_err(|e| format!("\"{name}\" could not be copied: {e}."))?;
    Ok(Saved {
        relative: relative_of(root, &target),
        key: looks_like_a_key(Path::new(&target_name)),
        name: target_name,
    })
}

/// The files under a dropped folder, hidden files and links left out.
fn files_under(dir: &Path, found: &mut Vec<(PathBuf, PathBuf)>, base: &Path, cut: &mut bool) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            files_under(&path, found, base, cut);
        } else if kind.is_file() {
            if found.len() >= MAX_FILES {
                *cut = true;
                return;
            }
            let inside = path.strip_prefix(base).unwrap_or(&path).to_path_buf();
            found.push((path, inside));
        }
    }
}

/// Copy what the learner dropped or picked into the study folder. Returns
/// what was saved, and a line for each thing that could not be.
pub fn copy_in(root: &Path, sources: &[PathBuf]) -> (Vec<Saved>, Vec<String>) {
    let mut saved = Vec::new();
    let mut problems = Vec::new();
    let materials = match make_dir(root, Path::new(MATERIALS)) {
        Ok(dir) => dir,
        Err(error) => return (saved, vec![error]),
    };
    for source in sources {
        let name = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        match fs::symlink_metadata(source) {
            Ok(meta) if meta.is_dir() => {
                let mut found = Vec::new();
                let mut cut = false;
                files_under(source, &mut found, source, &mut cut);
                if found.is_empty() {
                    problems.push(format!("\"{name}\" has no files in it."));
                    continue;
                }
                let folder = free_name(&materials, &clean_name(&name));
                for (path, inside) in found {
                    let parents = inside.parent().map(Path::to_path_buf).unwrap_or_default();
                    let mut relative = PathBuf::from(MATERIALS).join(&folder);
                    for part in parents.components() {
                        relative.push(clean_name(&part.as_os_str().to_string_lossy()));
                    }
                    let outcome = make_dir(root, &relative).and_then(|dir| {
                        let own = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        copy_file(root, &dir, &path, &own)
                    });
                    match outcome {
                        Ok(done) => saved.push(done),
                        Err(error) => problems.push(error),
                    }
                }
                if cut {
                    problems.push(format!(
                        "\"{name}\" has more than {MAX_FILES} files, so only the first {MAX_FILES} were added."
                    ));
                }
            }
            Ok(_) => match copy_file(root, &materials, source, &name) {
                Ok(done) => saved.push(done),
                Err(error) => problems.push(error),
            },
            Err(_) => problems.push(format!("\"{name}\" could not be read.")),
        }
    }
    (saved, problems)
}

/// Save pasted text as a text file.
pub fn save_text(root: &Path, text: &str) -> Result<Saved, String> {
    if text.trim().is_empty() {
        return Err("There is no text to save.".into());
    }
    if text.len() > MAX_PASTED_BYTES {
        return Err("That is too much text to paste. Save it as a file and add the file.".into());
    }
    let materials = make_dir(root, Path::new(MATERIALS))?;
    let stamp = Local::now().format("%Y-%m-%d %H-%M-%S");
    let name = free_name(&materials, &format!("Pasted text {stamp}.txt"));
    let target = materials.join(&name);
    fs::write(&target, text).map_err(|e| format!("The text could not be saved: {e}."))?;
    Ok(Saved {
        relative: relative_of(root, &target),
        key: false,
        name,
    })
}

fn extension_of(name: &str) -> String {
    split_name(name).1.to_ascii_lowercase()
}

/// One plain line about a file search will not read, or read only in part.
fn search_line(saved: &Saved, verdict: &Value) -> Option<String> {
    let name = &saved.name;
    let reason = verdict["reason"].as_str();
    let searchable = verdict["searchable"].as_bool().unwrap_or(false);
    let line = match reason {
        None if searchable => return None,
        Some("scanned") => format!("\"{name}\" is a scan with no text, so its text cannot be searched."),
        Some("scan_converted") => {
            format!("\"{name}\" is a scan with no text. The conversion worker will try to read it.")
        }
        Some("partly_scanned") => format!(
            "{} of the {} pages of \"{name}\" have no text and cannot be searched.",
            verdict["pages_without_text"].as_u64().unwrap_or(0),
            verdict["pages"].as_u64().unwrap_or(0)
        ),
        Some("name_word") => format!(
            "Search leaves out \"{name}\" because its name has the word \"{}\". Rename it and add it again.",
            verdict["word"].as_str().unwrap_or("key")
        ),
        Some("unsupported_type") => {
            let ext = extension_of(name);
            if ext.is_empty() {
                format!("Search does not read \"{name}\". A PDF or a text file can be searched.")
            } else {
                format!("Search does not read .{ext} files, so \"{name}\" cannot be searched. A PDF of it can be.")
            }
        }
        Some("secret_content") => format!(
            "Search leaves out \"{name}\" because its text looks like it holds a password or key."
        ),
        Some("too_large") => format!("\"{name}\" is too large for search."),
        Some("unreadable") => format!("The text of \"{name}\" could not be read, so it cannot be searched."),
        _ => format!(
            "\"{name}\": {}",
            verdict["detail"].as_str().unwrap_or("search will not read it.")
        ),
    };
    Some(line)
}

/// How many names a listing carries. The count of files is always whole.
const NAMES_LISTED: usize = 20;
/// The most entries a listing looks at, so a huge folder cannot hold it up.
const MAX_LOOKED_AT: usize = 5000;

/// What the course's material folder holds, as the course page says it.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct Listed {
    /// The names of the most recent files, newest first, answer keys left
    /// out. A file in a folder is named from the material folder, as
    /// "Week 3/slides.pdf".
    pub names: Vec<String>,
    /// How many files there are that are not answer keys.
    pub files: usize,
    /// How many files look like answer keys. They are counted and never named.
    pub keys: usize,
}

fn walk(
    dir: &Path,
    below: &Path,
    found: &mut Vec<(std::time::SystemTime, PathBuf)>,
    looked: &mut usize,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        if *looked >= MAX_LOOKED_AT {
            return;
        }
        *looked += 1;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            walk(&path, below, found, looked);
        } else if kind.is_file() {
            let when = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            found.push((
                when,
                path.strip_prefix(below).unwrap_or(&path).to_path_buf(),
            ));
        }
    }
}

/// The files in the course's material folder (`Study/materials`, where the
/// window puts what it adds), the most recent first. The window's one rule for
/// answer keys decides which are counted and not named: [`looks_like_a_key`],
/// asked of the path from the study root, as it is when a source is opened.
/// Hidden files and links are not listed, as when a folder is dropped.
pub fn list(root: &Path) -> Listed {
    let below = root.join(MATERIALS);
    let mut found = Vec::new();
    walk(&below, &below, &mut found, &mut 0);
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut listed = Listed::default();
    for (_, inside) in found {
        if looks_like_a_key(&Path::new(MATERIALS).join(&inside)) {
            listed.keys += 1;
            continue;
        }
        listed.files += 1;
        if listed.names.len() < NAMES_LISTED {
            listed.names.push(inside.to_string_lossy().into_owned());
        }
    }
    listed
}

/// Word what was done. `search` is the runtime's answer for each saved file
/// that is not an answer key, in order, or `None` when it could not say.
pub fn report(
    saved: &[Saved],
    problems: Vec<String>,
    search: Option<&[Value]>,
    pasted: bool,
) -> Added {
    let mut lines = Vec::new();
    let named: Vec<&Saved> = saved.iter().filter(|s| !s.key).collect();
    let keys = saved.len() - named.len();
    match named.as_slice() {
        [] => {}
        [only] if pasted => lines.push(format!("Saved your pasted text as \"{}\".", only.name)),
        [only] => lines.push(format!("Added \"{}\" to {MATERIALS}.", only.name)),
        many => lines.push(format!("Added {} files to {MATERIALS}.", many.len())),
    }
    if keys > 0 {
        lines.push(if keys == 1 {
            "1 file looks like an answer key. It is added, and this window keeps it closed.".to_owned()
        } else {
            format!("{keys} files look like answer keys. They are added, and this window keeps them closed.")
        });
    }
    if !named.is_empty() {
        match search {
            Some(verdicts) => {
                let problems_found: Vec<String> = named
                    .iter()
                    .zip(verdicts)
                    .filter_map(|(file, verdict)| search_line(file, verdict))
                    .collect();
                if problems_found.is_empty() {
                    lines.push(
                        if named.len() == 1 {
                            "Search can read it."
                        } else {
                            "Search can read all of it."
                        }
                        .to_owned(),
                    );
                } else {
                    let more = problems_found.len().saturating_sub(LINES_BEFORE_MORE);
                    lines.extend(problems_found.into_iter().take(LINES_BEFORE_MORE));
                    if more > 0 {
                        lines.push(format!(
                            "{more} more {} that search cannot read in full.",
                            if more == 1 { "file" } else { "files" }
                        ));
                    }
                }
            }
            None => lines.push(
                "This version of Magnesium cannot say whether search can read what you added."
                    .to_owned(),
            ),
        }
    }
    lines.extend(problems);
    Added {
        saved: named.iter().map(|s| s.relative.clone()).collect(),
        keys,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn a_file_is_copied_under_its_own_name_and_never_overwrites_another() {
        let root = root();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("Week 4 slides (v2).pdf");
        fs::write(&source, b"%PDF-1.4").unwrap();
        let (first, problems) = copy_in(root.path(), std::slice::from_ref(&source));
        let (second, _) = copy_in(root.path(), std::slice::from_ref(&source));
        assert!(problems.is_empty());
        assert_eq!(first[0].relative, "Study/materials/Week 4 slides (v2).pdf");
        assert_eq!(
            second[0].relative,
            "Study/materials/Week 4 slides (v2) 2.pdf"
        );
        assert!(root.path().join(&first[0].relative).is_file());
        assert!(source.is_file(), "the original stays where it was");
        assert!(!first[0].key);
    }

    #[test]
    fn the_material_goes_where_search_reads_and_not_in_the_control_folder() {
        assert!(MATERIALS.starts_with("Study/"));
        assert!(!MATERIALS.contains(".magnesium"));
    }

    #[test]
    fn a_file_that_looks_like_an_answer_key_is_saved_and_never_named() {
        let root = root();
        let outside = tempfile::tempdir().unwrap();
        let key = outside.path().join("Midterm answer key.pdf");
        let slides = outside.path().join("Slides.pdf");
        fs::write(&key, b"x").unwrap();
        fs::write(&slides, b"x").unwrap();
        let (saved, _) = copy_in(root.path(), &[key, slides]);
        assert_eq!(saved.iter().filter(|s| s.key).count(), 1);
        assert!(root
            .path()
            .join("Study/materials/Midterm answer key.pdf")
            .is_file());

        let verdicts = [json!({"searchable": true, "reason": null})];
        let added = report(&saved, Vec::new(), Some(&verdicts), false);
        assert_eq!(added.keys, 1);
        assert_eq!(added.saved, ["Study/materials/Slides.pdf"]);
        let said = added.lines.join("\n");
        assert!(!said.contains("answer key.pdf"), "{said}");
        assert!(said.contains("1 file looks like an answer key"), "{said}");
        assert!(said.contains("Slides.pdf"), "{said}");
    }

    fn put(root: &Path, relative: &str, minutes_ago: u64) {
        let path = root.join(MATERIALS).join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(minutes_ago * 60);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn the_listing_names_files_newest_first_and_counts_an_answer_key_without_naming_it() {
        let root = root();
        put(root.path(), "week2-sampling.md", 300);
        put(root.path(), "week3-confidence-intervals.md", 5);
        put(root.path(), "Midterm answer key.pdf", 1);
        put(root.path(), "week1-intro.pdf", 900);
        put(root.path(), "Week 4/slides.pdf", 60);
        // The key rule asks the folder names too, as when a source is opened.
        put(root.path(), "Solutions/PS1.pdf", 2);
        let listed = list(root.path());
        assert_eq!(
            listed.names,
            [
                "week3-confidence-intervals.md",
                "Week 4/slides.pdf",
                "week2-sampling.md",
                "week1-intro.pdf",
            ]
        );
        assert_eq!(listed.files, 4);
        assert_eq!(listed.keys, 2);
        let said = format!("{listed:?}");
        assert!(!said.contains("answer key"), "{said}");
        assert!(!said.contains("PS1"), "{said}");
    }

    #[test]
    fn the_listing_leaves_out_hidden_files_and_links_and_says_none_for_no_folder() {
        let root = root();
        assert_eq!(list(root.path()), Listed::default());
        put(root.path(), "notes.txt", 3);
        put(root.path(), ".DS_Store", 1);
        put(root.path(), ".hidden/inside.txt", 1);
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("elsewhere.txt"), b"x").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                outside.path().join("elsewhere.txt"),
                root.path().join(MATERIALS).join("linked.txt"),
            )
            .unwrap();
            std::os::unix::fs::symlink(outside.path(), root.path().join(MATERIALS).join("door"))
                .unwrap();
        }
        let listed = list(root.path());
        assert_eq!(listed.names, ["notes.txt"]);
        assert_eq!((listed.files, listed.keys), (1, 0));
    }

    #[test]
    fn a_long_listing_names_the_newest_and_counts_them_all() {
        let root = root();
        for n in 0..30 {
            put(root.path(), &format!("part{n:02}.txt"), n + 1);
        }
        let listed = list(root.path());
        assert_eq!(listed.files, 30);
        assert_eq!(listed.names.len(), NAMES_LISTED);
        assert_eq!(listed.names[0], "part00.txt");
    }

    #[test]
    fn what_the_window_adds_is_what_the_listing_shows() {
        let root = root();
        let outside = tempfile::tempdir().unwrap();
        for name in ["Slides.pdf", "Midterm answer key.pdf"] {
            fs::write(outside.path().join(name), b"x").unwrap();
        }
        let (saved, _) = copy_in(
            root.path(),
            &[
                outside.path().join("Slides.pdf"),
                outside.path().join("Midterm answer key.pdf"),
            ],
        );
        assert_eq!(saved.len(), 2);
        let listed = list(root.path());
        assert_eq!(listed.names, ["Slides.pdf"]);
        assert_eq!(listed.keys, 1);
    }

    #[test]
    fn a_dropped_folder_keeps_its_shape_and_leaves_out_hidden_files_and_links() {
        let root = root();
        let outside = tempfile::tempdir().unwrap();
        let folder = outside.path().join("Stats 101");
        fs::create_dir_all(folder.join("week1")).unwrap();
        fs::write(folder.join("week1/a.pdf"), b"x").unwrap();
        fs::write(folder.join("b.md"), b"x").unwrap();
        fs::write(folder.join(".DS_Store"), b"x").unwrap();
        std::os::unix::fs::symlink("/etc/hosts", folder.join("hosts")).unwrap();
        let (saved, problems) = copy_in(root.path(), &[folder]);
        assert!(problems.is_empty(), "{problems:?}");
        let mut got: Vec<_> = saved.iter().map(|s| s.relative.as_str()).collect();
        got.sort();
        assert_eq!(
            got,
            [
                "Study/materials/Stats 101/b.md",
                "Study/materials/Stats 101/week1/a.pdf"
            ]
        );
    }

    #[test]
    fn nothing_is_written_through_a_link_or_outside_the_study_folder() {
        let root = root();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("Study")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("Study/materials")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("a.pdf");
        fs::write(&source, b"x").unwrap();
        let (saved, problems) = copy_in(root.path(), &[source]);
        assert!(saved.is_empty());
        assert!(problems[0].contains("is a link"), "{problems:?}");
        assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
        assert!(save_text(root.path(), "words").is_err());
    }

    #[test]
    fn a_name_cannot_climb_out_of_the_material_folder() {
        let root = root();
        let outside = tempfile::tempdir().unwrap();
        // A file name cannot hold a slash on disk, but the cleaner must cope with one.
        assert_eq!(clean_name("../../etc/passwd"), "-..-etc-passwd");
        assert_eq!(clean_name(".hidden"), "hidden");
        assert_eq!(clean_name(""), "file");
        assert_eq!(clean_name("a\nb.txt"), "a-b.txt");
        let source = outside.path().join(".hidden.md");
        fs::write(&source, b"x").unwrap();
        let (saved, _) = copy_in(root.path(), &[source]);
        assert_eq!(saved[0].relative, "Study/materials/hidden.md");
        let long = format!("{}.pdf", "n".repeat(300));
        assert!(clean_name(&long).ends_with(".pdf"));
        assert!(clean_name(&long).chars().count() <= 120);
    }

    #[test]
    fn a_file_over_200_megabytes_is_not_copied() {
        let root = root();
        let outside = tempfile::tempdir().unwrap();
        let big = outside.path().join("lecture.mov");
        fs::File::create(&big)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        let (saved, problems) = copy_in(root.path(), &[big]);
        assert!(saved.is_empty());
        assert!(problems[0].contains("larger than 200 MB"), "{problems:?}");
        assert!(!root.path().join("Study/materials/lecture.mov").exists());
    }

    #[test]
    fn pasted_text_is_saved_as_a_text_file() {
        let root = root();
        let saved = save_text(root.path(), "Bayes' rule: P(A|B) = ...").unwrap();
        assert!(
            saved.relative.starts_with("Study/materials/Pasted text "),
            "{saved:?}"
        );
        assert!(saved.relative.ends_with(".txt"));
        assert_eq!(
            fs::read_to_string(root.path().join(&saved.relative)).unwrap(),
            "Bayes' rule: P(A|B) = ..."
        );
        assert!(save_text(root.path(), "  \n ").is_err());
        let added = report(
            &[saved],
            Vec::new(),
            Some(&[json!({"searchable": true})]),
            true,
        );
        assert!(
            added.lines[0].starts_with("Saved your pasted text as \"Pasted text "),
            "{added:?}"
        );
    }

    #[test]
    fn what_search_cannot_read_is_said_in_one_plain_line_each() {
        let saved: Vec<Saved> = [
            "scan.pdf",
            "tokens notes.md",
            "deck.pptx",
            "half.pdf",
            "fine.pdf",
        ]
        .iter()
        .map(|name| Saved {
            relative: format!("Study/materials/{name}"),
            name: (*name).to_owned(),
            key: false,
        })
        .collect();
        let verdicts = [
            json!({"searchable": false, "reason": "scanned"}),
            json!({"searchable": false, "reason": "name_word", "word": "tokens"}),
            json!({"searchable": false, "reason": "unsupported_type"}),
            json!({"searchable": true, "reason": "partly_scanned", "pages": 20, "pages_without_text": 3}),
            json!({"searchable": true, "reason": null}),
        ];
        let added = report(&saved, Vec::new(), Some(&verdicts), false);
        assert_eq!(
            added.lines,
            [
                "Added 5 files to Study/materials.",
                "\"scan.pdf\" is a scan with no text, so its text cannot be searched.",
                "Search leaves out \"tokens notes.md\" because its name has the word \"tokens\". Rename it and add it again.",
                "Search does not read .pptx files, so \"deck.pptx\" cannot be searched. A PDF of it can be.",
                "3 of the 20 pages of \"half.pdf\" have no text and cannot be searched.",
            ]
        );
        assert_eq!(added.saved.len(), 5);
    }

    #[test]
    fn many_unreadable_files_are_summed_up_and_an_old_runtime_is_admitted() {
        let saved: Vec<Saved> = (0..7)
            .map(|n| Saved {
                relative: format!("Study/materials/s{n}.pdf"),
                name: format!("s{n}.pdf"),
                key: false,
            })
            .collect();
        let scan = json!({"searchable": false, "reason": "scanned"});
        let verdicts = vec![scan; 7];
        let added = report(&saved, Vec::new(), Some(&verdicts), false);
        assert_eq!(added.lines.len(), 1 + LINES_BEFORE_MORE + 1);
        assert_eq!(
            added.lines.last().unwrap(),
            "3 more files that search cannot read in full."
        );

        let none = report(&saved[..1], Vec::new(), None, false);
        assert!(
            none.lines
                .iter()
                .any(|l| l.contains("cannot say whether search")),
            "{none:?}"
        );

        let all = report(
            &saved[..1],
            vec!["\"x\" could not be read.".into()],
            Some(&[json!({"searchable": true})]),
            false,
        );
        assert_eq!(
            all.lines,
            [
                "Added \"s0.pdf\" to Study/materials.",
                "Search can read it.",
                "\"x\" could not be read."
            ]
        );
    }
}
