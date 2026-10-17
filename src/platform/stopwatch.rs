//! Simple elapsed-time helper. Source: `OSTPlatform/include/Stopwatch.h`
//!
//! Rust's `Instant` is a monotonic clock with the same semantics as
//! `std::chrono::steady_clock`.

use std::time::Instant;

pub struct Stopwatch {
    start: Instant,
}

impl Stopwatch {
    pub fn new() -> Self {
        Self { start: Instant::now() }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }
}

impl Default for Stopwatch {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn new_is_monotonic_nonzero_after_sleep() {
        let s = Stopwatch::new();
        sleep(Duration::from_millis(5));
        let elapsed = s.elapsed_ms();
        assert!(elapsed >= 4.0, "expected >= 4 ms, got {}", elapsed);
        assert!(elapsed < 500.0, "suspiciously large elapsed: {}", elapsed);
    }

    #[test]
    fn elapsed_grows() {
        let s = Stopwatch::new();
        let a = s.elapsed_ms();
        sleep(Duration::from_millis(2));
        let b = s.elapsed_ms();
        assert!(b > a);
    }
}
