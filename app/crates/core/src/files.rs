use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::Local;
use serde::Serialize;

use crate::paths;
use crate::study::looks_like_a_key;

const IMAGES: [&str; 9] = [
    "heic", "heif", "jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp",
];
const MAX_BYTES: u64 = 200 * 1024 * 1024;
const DOCUMENTS: [&str; 11] = [
    "pdf", "png", "jpg", "jpeg", "gif", "webp", "md", "tex", "txt", "csv", "html",
];

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn safe_name(text: &str) -> String {
    let name: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_owned();
    if name.is_empty() {
        "work".to_owned()
    } else {
        name.chars().take(48).collect()
    }
}

fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem}-{n}.{ext}")))
        .find(|candidate| !candidate.exists())
        .unwrap_or(first)
}

pub fn hand_in(root: &Path, group: &str, source: &Path) -> Result<String, String> {
    let ext = extension(source);
    let is_image = IMAGES.contains(&ext.as_str());
    if !is_image && ext != "pdf" {
        return Err("Attach a photo or a PDF.".into());
    }
    let size = fs::metadata(source)
        .map_err(|_| "That file could not be read.".to_owned())?
        .len();
    if size > MAX_BYTES {
        return Err("That file is larger than 200 MB.".into());
    }
    let day = Local::now().format("%Y-%m-%d").to_string();
    let relative = PathBuf::from("Study")
        .join("handed-in")
        .join(safe_name(group))
        .join(day);
    let dir = root.join(&relative);
    fs::create_dir_all(&dir).map_err(|e| format!("This space could not be written to: {e}."))?;
    let stem = safe_name(
        &source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    let target = if is_image {
        let target = free_path(&dir, &stem, "jpg");
        if !shrink(source, &target) {
            let target = free_path(&dir, &stem, &ext);
            fs::copy(source, &target).map_err(|e| format!("The file could not be copied: {e}."))?;
            target
        } else {
            target
        }
    } else {
        let target = free_path(&dir, &stem, "pdf");
        fs::copy(source, &target).map_err(|e| format!("The file could not be copied: {e}."))?;
        target
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(relative.join(name).to_string_lossy().into_owned())
}

pub fn hand_in_bytes(root: &Path, group: &str, name: &str, bytes: &[u8]) -> Result<String, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("That image is larger than 200 MB.".into());
    }
    let ext = extension(Path::new(name));
    let ext = if IMAGES.contains(&ext.as_str()) || ext == "pdf" {
        ext
    } else {
        "png".to_owned()
    };
    let stem = safe_name(
        &Path::new(name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "pasted".into()),
    );
    let scratch = std::env::temp_dir().join(format!(
        "magnesium-paste-{}-{}",
        std::process::id(),
        Local::now().timestamp_millis()
    ));
    fs::create_dir_all(&scratch).map_err(|e| format!("The image could not be kept: {e}."))?;
    let file = scratch.join(format!(
        "{}.{ext}",
        if stem.is_empty() { "pasted" } else { &stem }
    ));
    let result = fs::write(&file, bytes)
        .map_err(|e| format!("The image could not be kept: {e}."))
        .and_then(|()| hand_in(root, group, &file));
    let _ = fs::remove_dir_all(&scratch);
    result
}

fn shrink(source: &Path, target: &Path) -> bool {
    let Some(sips) = paths::find_program("sips").or_else(|| {
        let system = PathBuf::from("/usr/bin/sips");
        system.is_file().then_some(system)
    }) else {
        return false;
    };
    let mut command = Command::new(sips);
    command
        .args([
            "-s",
            "format",
            "jpeg",
            "-s",
            "formatOptions",
            "80",
            "-Z",
            "2200",
        ])
        .arg(source)
        .arg("--out")
        .arg(target);
    paths::run(command, Duration::from_secs(60)).is_ok() && target.is_file()
}

pub fn inside(root: &Path, path: &str) -> Result<PathBuf, String> {
    let given = Path::new(path.trim());
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        root.join(given)
    };
    let resolved = joined
        .canonicalize()
        .map_err(|_| "That file does not exist.".to_owned())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if !resolved.starts_with(&root) {
        return Err("That file is outside this space.".into());
    }
    Ok(resolved)
}

pub fn openable(root: &Path, path: &str) -> Result<PathBuf, String> {
    let resolved = inside(root, path)?;
    if looks_like_a_key(&resolved) {
        return Err("That looks like an answer key, so the window leaves it closed.".into());
    }
    if !resolved.is_file() || !DOCUMENTS.contains(&extension(&resolved).as_str()) {
        return Err(
            "The window only opens documents, web pages and images: PDF, HTML, PNG, JPEG, GIF, WebP, Markdown, TeX, text and CSV."
                .into(),
        );
    }
    Ok(resolved)
}

pub fn open(root: &Path, path: &str) -> Result<(), String> {
    launch(&openable(root, path)?)
}

pub fn launch(resolved: &Path) -> Result<(), String> {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = Command::new(opener)
        .arg(resolved)
        .spawn()
        .map_err(|e| format!("The file could not be opened: {e}."))?;
    if cfg!(target_os = "macos") {
        let _ = child.wait();
    } else {
        std::thread::spawn(move || child.wait());
    }
    Ok(())
}

const TEXT_SOURCES: [&str; 8] = ["txt", "md", "tex", "csv", "html", "htm", "json", "rst"];
const MAX_TEXT_BYTES: u64 = 1024 * 1024;
const MAX_PDF_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Source {
    pub name: String,
    pub kind: &'static str,
    pub text: Option<String>,
    pub truncated: bool,
    pub image: Option<String>,
}

fn source_file(root: &Path, path: &str) -> Result<(PathBuf, String), String> {
    let given = path.trim();
    if !Path::new(given).is_absolute() && looks_like_a_key(Path::new(given)) {
        return Err("That looks like an answer key, so the window leaves it closed.".into());
    }
    let resolved = inside(root, given)?;
    let canonical_root = root.canonicalize().map_err(|e| e.to_string())?;
    let relative = resolved.strip_prefix(&canonical_root).unwrap_or(&resolved);
    if looks_like_a_key(relative) {
        return Err("That looks like an answer key, so the window leaves it closed.".into());
    }
    if !resolved.is_file() {
        return Err("That is not a file.".into());
    }
    Ok((resolved.clone(), extension(&resolved)))
}

pub fn shareable(root: &Path, path: &str) -> Result<PathBuf, String> {
    source_file(root, path).map(|(file, _)| file)
}

pub fn source(root: &Path, path: &str) -> Result<Source, String> {
    let (file, ext) = source_file(root, path)?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let head = |kind| Source {
        name: name.clone(),
        kind,
        text: None,
        truncated: false,
        image: None,
    };
    if ext == "pdf" {
        let size = fs::metadata(&file).map_err(|e| e.to_string())?.len();
        if size > MAX_PDF_BYTES {
            return Err("That PDF is larger than 64 MB.".into());
        }
        return Ok(head("pdf"));
    }
    if TEXT_SOURCES.contains(&ext.as_str()) {
        let size = fs::metadata(&file).map_err(|e| e.to_string())?.len();
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(
                fs::File::open(&file).map_err(|e| e.to_string())?,
                MAX_TEXT_BYTES,
            ),
            &mut bytes,
        )
        .map_err(|e| e.to_string())?;
        return Ok(Source {
            text: Some(String::from_utf8_lossy(&bytes).into_owned()),
            truncated: size > MAX_TEXT_BYTES,
            ..head("text")
        });
    }
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
        let shown = file
            .strip_prefix(root.canonicalize().map_err(|e| e.to_string())?)
            .unwrap_or(&file);
        return Ok(Source {
            image: Some(image(root, &shown.to_string_lossy())?),
            ..head("image")
        });
    }
    Err("The window shows PDFs, text and images beside a lesson. Other files stay closed.".into())
}

pub fn source_pdf(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let (file, ext) = source_file(root, path)?;
    if ext != "pdf" {
        return Err("That is not a PDF.".into());
    }
    if fs::metadata(&file).map_err(|e| e.to_string())?.len() > MAX_PDF_BYTES {
        return Err("That PDF is larger than 64 MB.".into());
    }
    let bytes = fs::read(&file).map_err(|_| "That file could not be read.".to_owned())?;
    if !bytes.starts_with(b"%PDF") {
        return Err("That file is not a PDF.".into());
    }
    Ok(bytes)
}

pub fn image(root: &Path, path: &str) -> Result<String, String> {
    let resolved = inside(root, path)?;
    if looks_like_a_key(&resolved) {
        return Err("That looks like an answer key.".into());
    }
    let mime = match extension(&resolved).as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => return Err("That is not an image the window can show.".into()),
    };
    if fs::metadata(&resolved).map_err(|e| e.to_string())?.len() > 20 * 1024 * 1024 {
        return Err("That image is larger than 20 MB.".into());
    }
    let bytes = fs::read(&resolved).map_err(|e| e.to_string())?;
    Ok(format!("data:{mime};base64,{}", base64(&bytes)))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn a_pdf_is_copied_into_the_study_folder() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let source = outside.path().join("My answers (1).pdf");
        fs::write(&source, b"%PDF-1.4").unwrap();
        let first = hand_in(root.path(), "Paper B", &source).unwrap();
        let second = hand_in(root.path(), "Paper B", &source).unwrap();
        assert!(first.starts_with("Study/handed-in/Paper-B/"));
        assert!(first.ends_with("My-answers--1.pdf"), "{first}");
        assert_ne!(first, second, "a second copy never overwrites the first");
        assert!(root.path().join(&first).is_file());
        assert!(source.is_file(), "the original stays where it was");
    }

    #[test]
    fn other_kinds_of_file_are_refused() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("notes.docx");
        fs::write(&source, b"x").unwrap();
        assert!(hand_in(root.path(), "work", &source).is_err());
    }

    #[test]
    fn a_path_outside_the_folder_is_refused_and_a_key_stays_closed() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.png"), b"x").unwrap();
        let escape = format!(
            "../{}/secret.png",
            outside.path().file_name().unwrap().to_string_lossy()
        );
        assert!(inside(root.path(), &escape).is_err());
        assert!(inside(
            root.path(),
            &outside.path().join("secret.png").to_string_lossy()
        )
        .is_err());

        fs::create_dir_all(root.path().join("Study/m/papers")).unwrap();
        fs::write(root.path().join("Study/m/papers/PaperB-key.pdf"), b"x").unwrap();
        let refused = open(root.path(), "Study/m/papers/PaperB-key.pdf").unwrap_err();
        assert!(refused.contains("answer key"));
    }

    #[test]
    fn a_large_image_is_refused_before_it_is_read() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("big.png");
        fs::File::create(&path)
            .unwrap()
            .set_len(21 * 1024 * 1024)
            .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
        let refused = image(root.path(), "big.png").unwrap_err();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(refused.contains("larger than 20 MB"), "{refused}");
    }

    #[test]
    fn only_documents_and_images_are_opened() {
        let root = tempfile::tempdir().unwrap();
        let notes = root.path().join("Study/notes");
        fs::create_dir_all(&notes).unwrap();
        for name in [
            "a.pdf",
            "b.PNG",
            "c.jpg",
            "d.jpeg",
            "e.gif",
            "f.webp",
            "g.md",
            "h.txt",
            "i.tex",
            "j.csv",
            "paper.html",
        ] {
            fs::write(notes.join(name), b"x").unwrap();
            let path = format!("Study/notes/{name}");
            assert!(openable(root.path(), &path).is_ok(), "{name} should open");
        }
        fs::create_dir_all(notes.join("Tool.app/Contents")).unwrap();
        fs::create_dir_all(notes.join("folder.pdf")).unwrap();
        for name in [
            "run.command",
            "run.sh",
            "script.js",
            "link.webloc",
            "flow.workflow",
            "README",
        ] {
            fs::write(notes.join(name), b"x").unwrap();
        }
        for name in [
            "run.command",
            "run.sh",
            "script.js",
            "link.webloc",
            "flow.workflow",
            "README",
            "Tool.app",
            "folder.pdf",
            ".",
        ] {
            let path = format!("Study/notes/{name}");
            let refused = openable(root.path(), &path).unwrap_err();
            assert!(refused.contains("only opens"), "{name}: {refused}");
        }
    }

    #[test]
    fn a_document_is_shared_as_its_own_file_and_never_a_key() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let papers = root.path().join("Study/midterm-2/papers");
        fs::create_dir_all(&papers).unwrap();
        fs::write(papers.join("Paper2-Medium.pdf"), b"%PDF-1.4").unwrap();
        fs::write(papers.join("Paper2-Medium-KEY.pdf"), b"%PDF-1.4").unwrap();
        fs::write(outside.path().join("secret.pdf"), b"%PDF-1.4").unwrap();
        std::os::unix::fs::symlink(
            papers.join("Paper2-Medium-KEY.pdf"),
            papers.join("innocent.pdf"),
        )
        .unwrap();

        let shared = shareable(root.path(), "Study/midterm-2/papers/Paper2-Medium.pdf").unwrap();
        assert_eq!(
            shared,
            papers.join("Paper2-Medium.pdf").canonicalize().unwrap()
        );
        for refused in [
            "Study/midterm-2/papers/Paper2-Medium-KEY.pdf",
            "Study/midterm-2/papers/innocent.pdf",
            "../secret.pdf",
            outside.path().join("secret.pdf").to_str().unwrap(),
            "Study/midterm-2/papers",
        ] {
            assert!(
                shareable(root.path(), refused).is_err(),
                "{refused} was shared"
            );
        }
    }

    #[test]
    fn a_cited_source_opens_inside_the_study_folder_and_never_a_key() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let dir = root.path().join("Study/stats");
        fs::create_dir_all(dir.join("keys")).unwrap();
        fs::write(dir.join("week4-slides.pdf"), b"%PDF-1.4 slides").unwrap();
        fs::write(dir.join("notes.md"), "# Bayes\nprior").unwrap();
        fs::write(dir.join("Midterm solutions.pdf"), b"%PDF-1.4").unwrap();
        fs::write(dir.join("answer-key.txt"), b"x").unwrap();
        fs::write(dir.join("keys/week4.pdf"), b"%PDF-1.4").unwrap();
        fs::write(dir.join("not-really.pdf"), b"plain text").unwrap();
        fs::write(outside.path().join("secret.pdf"), b"%PDF-1.4").unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.pdf"), dir.join("link.pdf"))
            .unwrap();
        std::os::unix::fs::symlink(dir.join("Midterm solutions.pdf"), dir.join("innocent.pdf"))
            .unwrap();
        std::os::unix::fs::symlink(dir.join("notes.md"), dir.join("the-key.md")).unwrap();

        let slides = source(root.path(), "Study/stats/week4-slides.pdf").unwrap();
        assert_eq!(
            (slides.kind, slides.name.as_str()),
            ("pdf", "week4-slides.pdf")
        );
        assert_eq!(
            source_pdf(root.path(), "Study/stats/week4-slides.pdf").unwrap(),
            b"%PDF-1.4 slides"
        );
        let notes = source(root.path(), "Study/stats/notes.md").unwrap();
        assert_eq!(
            (notes.kind, notes.text.as_deref()),
            ("text", Some("# Bayes\nprior"))
        );
        assert!(source_pdf(root.path(), "Study/stats/notes.md").is_err());
        assert!(source_pdf(root.path(), "Study/stats/not-really.pdf").is_err());

        for key in [
            "Study/stats/Midterm solutions.pdf",
            "Study/stats/answer-key.txt",
            "Study/stats/keys/week4.pdf",
            "Study/stats/innocent.pdf",
            "Study/stats/the-key.md",
            "study/stats/ANSWER-KEY.txt",
        ] {
            let refused = source(root.path(), key).unwrap_err();
            let refused_pdf = source_pdf(root.path(), key).unwrap_err();
            assert!(
                refused.contains("answer key") || refused.contains("does not exist"),
                "{key}: {refused}"
            );
            assert!(
                refused_pdf.contains("answer key")
                    || refused_pdf.contains("does not exist")
                    || refused_pdf.contains("not a PDF"),
                "{key}: {refused_pdf}"
            );
        }
        assert!(source(root.path(), "Study/stats/innocent.pdf")
            .unwrap_err()
            .contains("answer key"));

        assert!(source(root.path(), "../secret.pdf").is_err());
        assert!(source(
            root.path(),
            &outside.path().join("secret.pdf").to_string_lossy()
        )
        .is_err());
        assert!(source(root.path(), "Study/stats/link.pdf")
            .unwrap_err()
            .contains("outside"));
        assert!(source_pdf(root.path(), "Study/stats/link.pdf").is_err());
        assert!(source(root.path(), "Study/stats/missing.pdf").is_err());
        assert!(source(root.path(), "Study/stats").is_err());
    }

    #[test]
    fn a_source_that_is_not_a_pdf_text_or_image_stays_closed() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("run.sh"), b"echo").unwrap();
        fs::write(root.path().join("deck.pptx"), b"x").unwrap();
        for name in ["run.sh", "deck.pptx"] {
            assert!(source(root.path(), name)
                .unwrap_err()
                .contains("Other files stay closed"));
        }
    }

    #[test]
    fn a_long_text_source_is_cut_at_one_megabyte() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("big.txt"),
            vec![b'a'; (MAX_TEXT_BYTES + 10) as usize],
        )
        .unwrap();
        let big = source(root.path(), "big.txt").unwrap();
        assert!(big.truncated);
        assert_eq!(big.text.unwrap().len() as u64, MAX_TEXT_BYTES);
    }
}
