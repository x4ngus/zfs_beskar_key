// ============================================================================
// src/cmd/mod.rs – command subsystem root
// ============================================================================
pub mod base; // core shell execution utilities (Cmd, OutputData)
pub mod doctor;
pub mod init; // zbk init // zbk doctor
pub mod repair; // shared repair helpers (units, etc.)
pub mod simulate; // ephemeral vault simulations
pub mod unlock; // zbk unlock

// Re-export common types for convenience:
pub use base::{Cmd, OutputData};
