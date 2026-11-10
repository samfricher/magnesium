pub const REQUEST_BYTES: usize = 1024 * 1024;
pub const EVENT_BYTES: usize = 256 * 1024;
pub const PACKET_BYTES: usize = 64 * 1024;
pub const DEFAULT_PACKET_BYTES: usize = 24 * 1024;
pub fn text(field: &str, value: &str, max: usize) -> super::Result<()> {
    if value.len() > max {
        return Err(super::LearnerError::new("field_too_large", field));
    }
    Ok(())
}
/// Cut `value` to at most `max` bytes at a character boundary, ending it in
/// "…" so the stored text shows the cut, and say in `notes` what was cut. A
/// tutor that wrote too much gets its record saved, not a failed save.
pub fn shorten(path: &str, value: &str, max: usize, notes: &mut Vec<String>) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let end = boundary(value, max.saturating_sub(MARK.len()));
    notes.push(format!(
        "{path} was {} bytes, over its {max}-byte limit: saved the first {end} bytes and cut the rest, which began \"{}\"",
        value.len(),
        preview(&value[end..])
    ));
    format!("{}{MARK}", &value[..end])
}
/// Keep the first `count` labels and cut each to `max` bytes, with one note
/// per kind of cut.
pub fn shorten_labels(
    path: &str,
    labels: &[String],
    count: usize,
    max: usize,
    notes: &mut Vec<String>,
) -> Vec<String> {
    if labels.len() > count {
        let cut = &labels[count..];
        let mut named: Vec<String> = cut
            .iter()
            .take(8)
            .map(|label| format!("\"{}\"", preview(label)))
            .collect();
        if cut.len() > 8 {
            named.push(format!("and {} more", cut.len() - 8));
        }
        notes.push(format!(
            "{path} had {} labels, over its limit of {count}: kept the first {count} and cut {}",
            labels.len(),
            named.join(", ")
        ));
    }
    let mut long = 0;
    let kept = labels
        .iter()
        .take(count)
        .map(|label| {
            if label.len() <= max {
                return label.clone();
            }
            long += 1;
            format!("{}{MARK}", &label[..boundary(label, max - MARK.len())])
        })
        .collect();
    if long > 0 {
        notes.push(format!(
            "{path}: cut {long} label(s) longer than {max} bytes to fit"
        ));
    }
    kept
}
const MARK: &str = "…";
fn boundary(value: &str, mut end: usize) -> usize {
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}
fn preview(text: &str) -> String {
    let mut chars = text.chars();
    let start: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        format!("{start}{MARK}")
    } else {
        start
    }
}
pub const TITLE_BYTES: usize = 256;
pub const SCOPE_BYTES: usize = 512;
pub const SUMMARY_BYTES: usize = 1024;
pub const REVIEW_NOTE_BYTES: usize = 1024;
pub const STATE_BYTES: usize = 16 * 1024;
pub const OBSERVATION_BYTES: usize = 8 * 1024;
pub const CONDITIONS_BYTES: usize = 2 * 1024;
pub const ASSISTANCE_BYTES: usize = 2 * 1024;
pub const CHECKPOINT_BYTES: usize = 16 * 1024;
pub const SESSION_GOAL_BYTES: usize = 2 * 1024;
pub const CHECKPOINT_FIELD_BYTES: usize = 2 * 1024;
pub const PLAN_BYTES: usize = 4 * 1024;
pub const TOPIC_COUNT: usize = 6;
pub const TOPIC_BYTES: usize = 128;
pub const CUE_COUNT: usize = 16;
pub const CUE_BYTES: usize = 256;
pub const OBSERVATIONS_PER_BATCH: usize = 16;
pub const MEMORY_EDITS_PER_BATCH: usize = 8;
pub const REVIEWS_PER_BATCH: usize = 16;
pub const DISPOSITIONS_PER_BATCH: usize = 32;
// Additional envelope bounds keep receipts and immutable support readable within
// the 64 KiB packet ceiling. Larger raw work remains accessible by event ranges.
pub const RAW_EVENTS_PER_BATCH: usize = 32;
pub const REFERENCE_BYTES: usize = 256;
pub const EVENT_METADATA_BYTES: usize = 16 * 1024;
pub const SOURCE_REFS_BYTES: usize = 8 * 1024;
pub const EXCERPTS_BYTES: usize = 16 * 1024;
