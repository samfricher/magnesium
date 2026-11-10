pub mod capture;
pub mod limits;
pub mod model;
pub mod read;
mod records;
pub mod search;
pub mod store;
pub mod update;
pub use model::*;
pub use store::LearnerStore;
/// Labels match whatever their case and spacing.
pub(crate) fn label_key(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
#[cfg(test)]
mod batch_key_tests;
#[cfg(test)]
mod parse_error_tests;
#[cfg(test)]
mod snapshot_consistency_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transaction_edge_tests;

#[cfg(test)]
mod read_admission_tests;

#[cfg(test)]
mod checkpoint_and_attribution_tests;

#[cfg(test)]
mod checkpoint_basis_bound_tests;

#[cfg(test)]
mod search_projection_tests;
