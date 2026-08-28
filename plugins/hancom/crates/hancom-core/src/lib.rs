//! Shared foundations for the OfficeCLI Hancom plugin family.
//!
//! The HWP/HWPX crate keeps compatibility re-exports while all Hancom-family
//! binaries share these protocol, model, container, and safety boundaries.

#![forbid(unsafe_code)]

pub mod budget;
pub mod container;
pub mod diagnostics;
pub mod emit;
pub mod error;
pub mod heartbeat;
pub mod model;
pub mod xml_encoding;
