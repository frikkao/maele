//! Maele's routing core.
//!
//! One Jev (TypeSafe System One) call per voice turn decides the capability and
//! tone; cheap rules and trigram similarity stand behind it as the fail-open
//! path. The point is the decision, not the answer.

pub mod cli;
pub mod config;
pub mod dummy;
pub mod jev;
pub mod keys;
pub mod log;
pub mod matcher;
pub mod policy;
pub mod providers;
pub mod questions;
pub mod router;

pub use config::{Config, ConfigError};
pub use jev::{JevError, JevResponse, SystemOne};
pub use policy::{Decision, Router};
