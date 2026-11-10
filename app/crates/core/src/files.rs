//! The learner's own files: handing in work, and opening a paper.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::Local;

use crate::paths;
use crate::study::looks_like_a_key;

const IMAGES: [&str; 9] = [
    "heic", "heif", "jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp",
];
const MAX_BYTES: u64 = 200 * 1024 * 1024;

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

/// A path that does not exist yet: `name.ext`, then `name-2.ext`, and so on.
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

/// Put a copy of the learner's photo or PDF in the study folder and return its
/// path inside the folder. A phone photo is often HEIC and several megabytes,
/// which the tutor reads slowly or not at all, so an image is saved as a JPEG
/// no larger than 2200 pixels a side.
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
    fs::create_dir_all(&dir)
        .map_err(|e| format!("The study folder could not be written to: {e}."))?;
    let stem = safe_name(
        &source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    );
    let target = if is_image {
        let target = free_path(&dir, &stem, "jpg");
        if !shrink(source, &target) {
            // No converter on this system. Keep the original, which the tutor
            // can still try to read.
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

/// Convert with `sips`, which every Mac has.
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

/// Resolve a path the tutor or the window named, and keep it inside the study
/// folder.
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
        return Err("That file is outside the study folder.".into());
    }
    Ok(resolved)
}

/// Open a paper or a source in the system's own viewer. An answer key is
/// refused: the key stays closed until the tutor has marked the work.
pub fn open(root: &Path, path: &str) -> Result<(), String> {
    let resolved = inside(root, path)?;
    if looks_like_a_key(&resolved) {
        return Err("That looks like an answer key, so the window leaves it closed.".into());
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(opener)
        .arg(&resolved)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("The file could not be opened: {e}."))
}

/// An image inside the study folder as a data address the page can show.
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
    let bytes = fs::read(&resolved).map_err(|e| e.to_string())?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err("That image is larger than 20 MB.".into());
    }
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
}
