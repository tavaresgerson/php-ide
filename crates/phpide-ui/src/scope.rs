//! Product scope for v1.0.
//!
//! Frozen features stay compiled but are hidden from the default build. Enable
//! them with `cargo run --features experimental`.

/// `true` when the `experimental` Cargo feature is enabled.
pub const EXPERIMENTAL: bool = cfg!(feature = "experimental");
