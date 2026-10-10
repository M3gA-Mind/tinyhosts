//! Compatibility re-exports of the shared hosting error vocabulary.
pub use tinyhosts_bus::error::{Error, Result};

#[cfg(test)]
#[path = "mod_tests.rs"]
mod test;
