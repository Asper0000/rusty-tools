//! Support helpers. `Fnv1aHash` and `Stopwatch`.

pub mod fnv;

pub use fnv::fnv1a_hash;
pub use crate::platform::stopwatch::Stopwatch;
