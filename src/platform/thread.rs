//! Thread helpers. Source: `OSTPlatform/Windows/Thread.cpp`
//!
//! `start_detached` uses `std::thread` (equivalent to `CreateThread` +
//! immediate `CloseHandle`). `current_native_thread_handle` calls
//! `GetCurrentThread` on Windows; the returned pseudo-handle is
//! process-scoped and does not need closing, matching the C++ use.

use std::ffi::c_void;

pub type NativeThreadHandle = *mut c_void;

pub fn current_native_thread_handle() -> NativeThreadHandle {
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::System::Threading::GetCurrentThread() as NativeThreadHandle
    }
    #[cfg(not(windows))]
    {
        std::ptr::null_mut()
    }
}

/// Run `entry` on a detached thread. Returns false if the thread could
/// not be spawned (OS resource exhaustion).
pub fn start_detached<F>(entry: F) -> bool
where
    F: FnOnce() -> u32 + Send + 'static,
{
    std::thread::Builder::new()
        .spawn(entry)
        .map(|_h| true)   // handle dropped -> thread detached
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn detached_thread_runs() {
        let flag = Arc::new(AtomicU32::new(0));
        let f = flag.clone();
        assert!(start_detached(move || { f.store(42, Ordering::SeqCst); 0 }));
        for _ in 0..50 {
            if flag.load(Ordering::SeqCst) == 42 { return; }
            sleep(Duration::from_millis(2));
        }
        panic!("detached thread did not run");
    }

    #[cfg(windows)]
    #[test]
    fn current_native_thread_handle_is_nonnull() {
        // GetCurrentThread returns a pseudo-handle, always non-null.
        let h = current_native_thread_handle();
        assert!(!h.is_null());
    }

    #[cfg(not(windows))]
    #[test]
    fn current_native_thread_handle_is_null() {
        assert!(current_native_thread_handle().is_null());
    }
}
