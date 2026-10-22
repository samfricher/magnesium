//! Bounded native-start provenance. Inactive learner text stays in the host's
//! existing message, never in a Magnesium file. The host carries a short MAC tag
//! and echoes the exact bytes only when explicitly starting learning. The tag
//! binds the workspace, conversation, turn marker and text digest; everything
//! but the text is recomputed or read back from the marker, so the model only
//! copies 32 hexadecimal characters.
use crate::{
    learner::{HostContext, RawEvent},
    workspace::Workspace,
};
use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use std::{
    borrow::Cow,
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const DOMAIN: &[u8] = b"magnesium/native-initial-capture/v2\0";
pub(crate) const TTL_MS: i64 = 5 * 60 * 1000;
/// Left-truncated HMAC-SHA256 tag (RFC 2104 section 5): 128 bits.
const TAG_BYTES: usize = 16;
const MAX_MARKERS: usize = 128;
const DIRECTORY: &str = "cache/native-capture";
type Signature = Hmac<Sha256>;
fn failure() -> String {
    // One message for every cause: callers learn nothing about which check failed.
    "invalid_native_capture: use the current unexpired native ticket and exact learner text, or explicit model-reported fallback. Copy the whole ticket character for character from this conversation's latest Magnesium prompt notice; it is long hexadecimal, so do not retype or shorten it".into()
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn workspace_digest(workspace: &Workspace) -> Result<[u8; 32], String> {
    let root = fs::canonicalize(workspace.root()).map_err(|e| e.to_string())?;
    Ok(digest(
        &serde_json::to_vec(&(
            workspace.id().map_err(|e| e.to_string())?,
            crate::workspace::native_path_key(&root),
        ))
        .map_err(|e| e.to_string())?,
    ))
}
fn host_digest(host: &HostContext) -> [u8; 32] {
    digest(
        &serde_json::to_vec(&[&host.host_kind, &host.host_thread_id])
            .expect("serialize native identity"),
    )
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if text.len() != TAG_BYTES * 2
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(failure());
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|p| {
            u8::from_str_radix(std::str::from_utf8(p).map_err(|_| failure())?, 16)
                .map_err(|_| failure())
        })
        .collect()
}
fn directory(workspace: &Workspace) -> Result<PathBuf, String> {
    workspace
        .private_dir(Path::new(DIRECTORY))
        .map_err(|e| e.to_string())
}
fn read_exact_file(path: &Path, workspace: &Workspace, size: usize) -> Result<Vec<u8>, String> {
    crate::workspace::reject_symlink_components(path, workspace.control())
        .map_err(|e| e.to_string())?;
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() || meta.len() != size as u64 {
        return Err("invalid native capture metadata".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.file_name().is_some_and(|name| name == "signing-key")
            && meta.permissions().mode() & 0o077 != 0
        {
            return Err("native capture key must be owner-private".into());
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(size as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() != size {
        return Err("invalid native capture metadata".into());
    }
    Ok(bytes)
}
fn lock(workspace: &Workspace, dir: &Path) -> Result<fs::File, String> {
    let path = dir.join("metadata.lock");
    crate::workspace::reject_symlink_components(&path, workspace.control())
        .map_err(|e| e.to_string())?;
    let file = crate::workspace::open_private_lock(&path).map_err(|e| e.to_string())?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(error) => {
                let e: std::io::Error = error.into();
                if e.kind() != std::io::ErrorKind::WouldBlock
                    || std::time::Instant::now() >= deadline
                {
                    return Err("native capture metadata busy".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
}
/// Normal initialization creates only a private random provenance key. This is
/// not a provider/account credential and grants no model or network access.
pub(crate) fn initialize(workspace: &Workspace) -> Result<(), String> {
    let dir = directory(workspace)?;
    let _guard = lock(workspace, &dir)?;
    let path = dir.join("signing-key");
    crate::workspace::reject_symlink_components(&path, workspace.control())
        .map_err(|e| e.to_string())?;
    if path.try_exists().map_err(|e| e.to_string())? {
        read_exact_file(&path, workspace, 32)?;
        return Ok(());
    }
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).map_err(|e| e.to_string())?;
    crate::workspace::write_private_atomic(&path, &key).map_err(|e| e.to_string())
}
fn key(workspace: &Workspace) -> Result<Vec<u8>, String> {
    read_exact_file(
        &workspace.control().join(DIRECTORY).join("signing-key"),
        workspace,
        32,
    )
}
fn marker_path(dir: &Path, host: &HostContext) -> PathBuf {
    dir.join(format!("{}.turn", hex(&host_digest(host))))
}
fn time(bytes: &[u8]) -> i64 {
    i64::from_be_bytes(bytes.try_into().expect("fixed ticket timestamp"))
}
fn prune(workspace: &Workspace, dir: &Path, now: i64) -> Result<usize, String> {
    let mut live = 0;
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() != 69
            || !name.ends_with(".turn")
            || !name.as_bytes()[..64]
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            continue;
        }
        let path = entry.path();
        let metadata = read_exact_file(&path, workspace, 48)?;
        if time(&metadata[40..48]) < now {
            fs::remove_file(path).map_err(|e| e.to_string())?
        } else {
            live += 1
        }
    }
    Ok(live)
}
/// Stable native external key: identical wording on different turns stays distinct.
pub(crate) fn event_key(native_turn: &str) -> String {
    format!("native:{}", hex(&digest(native_turn.as_bytes())))
}
pub(crate) fn invalidate(workspace: &Workspace, host: &HostContext) -> Result<(), String> {
    let dir = directory(workspace)?;
    let _guard = lock(workspace, &dir)?;
    let path = marker_path(&dir, host);
    crate::workspace::reject_symlink_components(&path, workspace.control())
        .map_err(|e| e.to_string())?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
/// Claude's desktop app sends pasted text between `<pasted_content id="…">`
/// and `</pasted_content id="…">` lines. The hook receives them, but the model
/// is told the learner never sees the ids, so a tutor echoes the learner's
/// text without the tags. Claude learner text is therefore certified and stored
/// without matched tag lines (a closing line repeats its opening line's id),
/// blank lines before the first line, or whitespace after the last, so the
/// hook's text and the echo agree; the first line keeps its indentation.
/// Every matched pair is removed in one pass, so applying this twice changes
/// nothing. Other hosts' text stays exact.
pub(crate) fn learner_text<'a>(host: &HostContext, text: &'a str) -> Cow<'a, str> {
    if host.host_kind != "claude" {
        return Cow::Borrowed(text);
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut open: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut removed = vec![false; lines.len()];
    for (index, line) in lines.iter().enumerate() {
        let line = line.trim();
        if let Some(id) = paste_tag_id(line, "<pasted_content id=\"") {
            open.entry(id).or_default().push(index);
        } else if let Some(id) = paste_tag_id(line, "</pasted_content id=\"")
            && let Some(start) = open.get_mut(id).and_then(Vec::pop)
        {
            removed[start] = true;
            removed[index] = true;
        }
    }
    if !removed.contains(&true) {
        return Cow::Borrowed(trim_blank_lines(text));
    }
    let kept: String = lines
        .iter()
        .zip(&removed)
        .filter(|(_, removed)| !**removed)
        .map(|(line, _)| *line)
        .collect();
    Cow::Owned(trim_blank_lines(&kept).to_owned())
}
fn trim_blank_lines(text: &str) -> &str {
    let text = text.trim_end();
    let blank = text.len() - text.trim_start().len();
    match text[..blank].rfind('\n') {
        Some(newline) => &text[newline + 1..],
        None => text,
    }
}
fn paste_tag_id<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let id = line.strip_prefix(prefix)?.strip_suffix("\">")?;
    let valid = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    valid.then_some(id)
}
/// Caller must first enforce native workspace binding and recording controls.
/// The ticket certifies `learner_text(host, text)`.
pub(crate) fn issue(
    workspace: &Workspace,
    host: &HostContext,
    native_turn: &str,
    text: &str,
    now: i64,
) -> Result<String, String> {
    let text = learner_text(host, text);
    if host.host_kind == "tool_only" || now < 0 || text.len() > crate::learner::limits::EVENT_BYTES
    {
        return Err(failure());
    }
    initialize(workspace)?;
    let dir = directory(workspace)?;
    let _guard = lock(workspace, &dir)?;
    let path = marker_path(&dir, host);
    let count = prune(workspace, &dir, now)?;
    let turn = digest(native_turn.as_bytes());
    let expiry = now.checked_add(TTL_MS).ok_or_else(failure)?;
    let (issued, expires) = if path.try_exists().map_err(|e| e.to_string())? {
        let current = read_exact_file(&path, workspace, 48)?;
        if current[..32] == turn {
            (time(&current[32..40]), time(&current[40..48]))
        } else {
            (now, expiry)
        }
    } else {
        if count >= MAX_MARKERS {
            return Err("native capture cache full; use labeled model-reported fallback".into());
        }
        (now, expiry)
    };
    let mut marker = turn.to_vec();
    marker.extend(issued.to_be_bytes());
    marker.extend(expires.to_be_bytes());
    crate::workspace::write_private_atomic(&path, &marker).map_err(|e| e.to_string())?;
    let mac = signature(workspace, host, &marker, &text)?;
    Ok(hex(&mac.finalize().into_bytes()[..TAG_BYTES]))
}
/// MAC over everything the ticket certifies. The text digest exists only
/// inside this computation, never in a file.
fn signature(
    workspace: &Workspace,
    host: &HostContext,
    marker: &[u8],
    text: &str,
) -> Result<Signature, String> {
    let mut mac = Signature::new_from_slice(&key(workspace)?).map_err(|_| failure())?;
    mac.update(DOMAIN);
    mac.update(&workspace_digest(workspace)?);
    mac.update(&host_digest(host));
    mac.update(marker);
    mac.update(&digest(text.as_bytes()));
    Ok(mac)
}
/// Verification yields only the exact observed learner event, as
/// `learner_text`. It certifies no interpretation, task completion,
/// independent success, or model judgment.
pub(crate) fn verify(
    workspace: &Workspace,
    host: &HostContext,
    ticket: &str,
    text: &str,
    now: i64,
) -> Result<RawEvent, String> {
    let text = learner_text(host, text);
    if host.host_kind == "tool_only" || text.len() > crate::learner::limits::EVENT_BYTES {
        return Err(failure());
    }
    let tag = unhex(ticket)?;
    // The current marker names the only turn this conversation can start from.
    let marker = read_exact_file(
        &marker_path(&workspace.control().join(DIRECTORY), host),
        workspace,
        48,
    )
    .map_err(|_| failure())?;
    let (issued, expires) = (time(&marker[32..40]), time(&marker[40..48]));
    if issued > now || expires < now || expires.checked_sub(issued) != Some(TTL_MS) {
        return Err(failure());
    }
    signature(workspace, host, &marker, &text)?
        .verify_truncated_left(&tag)
        .map_err(|_| failure())?;
    Ok(RawEvent {
        external_event_key: format!("native:{}", hex(&marker[..32])),
        speaker: crate::learner::Speaker::Learner,
        kind: "message".into(),
        text: text.into_owned(),
        metadata: Some(crate::learner::EventMetadata::default()),
        occurred_at_ms: None,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard_hmac_sha256_rfc4231_vector() {
        let mut mac = Signature::new_from_slice(&[0x0b; 20]).unwrap();
        mac.update(b"Hi There");
        assert_eq!(
            hex(&mac.finalize().into_bytes()),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    fn fixture() -> (PathBuf, Workspace, HostContext) {
        let root =
            std::env::temp_dir().join(crate::learner::store::new_id("native-ticket").unwrap());
        fs::create_dir_all(root.join(crate::workspace::CONTROL_DIR)).unwrap();
        fs::write(
            root.join(crate::workspace::CONTROL_DIR).join("config.toml"),
            "schema_version = 1\nworkspace_id = \"native-ticket\"\n",
        )
        .unwrap();
        let workspace = Workspace::discover(&root).unwrap().unwrap();
        let host = HostContext {
            host_kind: "claude".into(),
            host_thread_id: "magnesium-h1:claude:conversation".into(),
        };
        (root, workspace, host)
    }

    #[test]
    fn short_ticket_verifies_only_the_exact_current_turn_text_and_conversation() {
        for host_kind in ["claude", "codex"] {
            for (text, altered) in [
                (
                    "Teach me confidence intervals.",
                    &["Teach me confidence intervals!"][..],
                ),
                (
                    "I cannot identify b; \u{2212}b is not +b. π ≠ x.",
                    &[
                        "I can identify b; \u{2212}b is not +b. π ≠ x.",
                        "I cannot identify b; -b is not +b. π ≠ x.",
                    ][..],
                ),
            ] {
                let (root, workspace, host) = fixture();
                let host = HostContext {
                    host_kind: host_kind.into(),
                    ..host
                };
                let ticket = issue(&workspace, &host, "turn-1", text, 1_000).unwrap();
                assert_eq!(ticket.len(), 32);
                assert!(ticket.bytes().all(|b| b.is_ascii_hexdigit()));
                // A retried prompt hook for the same turn reissues the same ticket.
                assert_eq!(
                    issue(&workspace, &host, "turn-1", text, 2_000).unwrap(),
                    ticket
                );

                let event = verify(&workspace, &host, &ticket, text, 1_500).unwrap();
                assert_eq!(event.text, text);
                assert_eq!(event.external_event_key, event_key("turn-1"));
                assert_eq!(event.occurred_at_ms, None);

                let other = HostContext {
                    host_thread_id: "magnesium-h1:claude:other".into(),
                    ..host.clone()
                };
                let flip = |index: usize| {
                    let mut bytes = ticket.clone().into_bytes();
                    bytes[index] = if bytes[index] == b'0' { b'1' } else { b'0' };
                    String::from_utf8(bytes).unwrap()
                };
                let (flipped_last, flipped_middle) = (flip(31), flip(20));
                let mut rejected = vec![
                    (&host, flipped_last.as_str(), text, 1_500),
                    (&host, flipped_middle.as_str(), text, 1_500),
                    (&host, &ticket[..31], text, 1_500),
                    (&other, ticket.as_str(), text, 1_500),
                    (&host, ticket.as_str(), text, 1_000 + TTL_MS + 1),
                    (&host, ticket.as_str(), text, 999),
                ];
                rejected.extend(
                    altered
                        .iter()
                        .map(|altered| (&host, ticket.as_str(), *altered, 1_500)),
                );
                for (host, ticket, text, now) in rejected {
                    assert_eq!(
                        verify(&workspace, host, ticket, text, now).err(),
                        Some(failure())
                    );
                }

                // A newer prompt replaces the marker, so the earlier ticket expires with it.
                issue(&workspace, &host, "turn-2", "Next question", 3_000).unwrap();
                assert_eq!(
                    verify(&workspace, &host, &ticket, text, 3_500).err(),
                    Some(failure())
                );
                fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn claude_paste_markup_is_certified_and_stored_as_the_learner_wrote_it() {
        let (root, workspace, host) = fixture();
        let words = "Why is E(w, b) a bowl?\n$$X^\\top X w = X^\\top t$$";
        // A whole-message paste from the Claude desktop app, byte for byte.
        let pasted =
            format!("\n\n<pasted_content id=\"b185\">\n{words}\n</pasted_content id=\"b185\">\n");
        let ticket = issue(&workspace, &host, "turn-1", &pasted, 1_000).unwrap();
        // The tutor may echo the learner's words alone or the whole prompt.
        for echo in [words, pasted.as_str()] {
            let event = verify(&workspace, &host, &ticket, echo, 1_500).unwrap();
            assert_eq!(event.text, words);
        }
        assert_eq!(
            verify(&workspace, &host, &ticket, "Why is E(w, b) a bowl?", 1_500).err(),
            Some(failure())
        );

        for (raw, stored) in [
            (
                "Also\n\n<pasted_content id=\"9bc0\">\nX = 1\n</pasted_content id=\"9bc0\">\n\nwhy?",
                "Also\n\nX = 1\n\nwhy?",
            ),
            (
                "<pasted_content id=\"a\">\n<pasted_content id=\"b\">\nx\n</pasted_content id=\"b\">\n</pasted_content id=\"a\">",
                "x",
            ),
            (
                "<pasted_content id=\"a\">\n</pasted_content id=\"a\">\nok\n",
                "ok",
            ),
            // Pasted code keeps its first line's indentation.
            (
                "\n\n<pasted_content id=\"c\">\n    if x:\n        return x\n</pasted_content id=\"c\">\n",
                "    if x:\n        return x",
            ),
            ("  indented\r\n\r\n", "  indented"),
            // Unmatched, inline or malformed tags are the learner's own text.
            (
                "<pasted_content id=\"a\">\nx\n</pasted_content id=\"b\">",
                "<pasted_content id=\"a\">\nx\n</pasted_content id=\"b\">",
            ),
            (
                "see <pasted_content id=\"a\">\nx\n</pasted_content id=\"a\">",
                "see <pasted_content id=\"a\">\nx\n</pasted_content id=\"a\">",
            ),
            (
                "<pasted_content id=\"a b\">\nx\n</pasted_content id=\"a b\">",
                "<pasted_content id=\"a b\">\nx\n</pasted_content id=\"a b\">",
            ),
        ] {
            assert_eq!(learner_text(&host, raw), stored);
            assert_eq!(learner_text(&host, stored), stored);
        }
        let codex = HostContext {
            host_kind: "codex".into(),
            ..host.clone()
        };
        assert_eq!(learner_text(&codex, &pasted), pasted.as_str());
        fs::remove_dir_all(root).unwrap();
    }
}
