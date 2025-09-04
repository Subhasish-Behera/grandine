pub mod block_processing;
pub mod epoch_processing;
pub mod slot_processing;
pub mod state_transition;

// Re-export the main functions used by combined.rs
pub use epoch_processing::{epoch_report, process_epoch};
pub use slot_processing::process_slots;