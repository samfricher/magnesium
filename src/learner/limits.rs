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
