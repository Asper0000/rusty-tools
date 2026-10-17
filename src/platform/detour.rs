//! Detour transaction API. Source: `OSTPlatform/Windows/Detour.cpp`
//!
//! Real implementation backed by Microsoft Detours via the FFI
//! declarations in `crate::platform::windows::detours_ffi`.
//!
//! The four public functions preserve the C++ API shape exactly. State
//! is per-thread: a transaction lives only on the thread that opened
//! it, matching Detours' own contract. `HookMacros.h` runs the
//! begin / attach / commit sequence from a single thread, so this is
//! never crossed in practice.
//!
//! ## Thread enrolment
//!
//! Detours requires the calling thread to be enrolled via
//! `DetourUpdateThread` before the first attach. On x64,
//! `DetourTransactionCommit` suspends and updates every other thread
//! automatically, so we enrol lazily: the first `attach` or `detach`
//! inside a transaction calls `DetourUpdateThread(GetCurrentThread())`
//! once. On x86 this would be insufficient - every thread would need
//! enrolling individually. This crate targets x64.

use std::cell::Cell;
use std::ffi::c_void;

#[cfg(windows)]
use super::windows::detours_ffi as ffi;

thread_local! {
    /// `true` when a transaction is open on the calling thread.
    static IN_TRANSACTION: Cell<bool> = const { Cell::new(false) };
    /// `true` once the calling thread has been enrolled in the current
    /// transaction. Reset by `commit_transaction`.
    static THREAD_ENROLLED: Cell<bool> = const { Cell::new(false) };
}

/// Begin a detour transaction on the calling thread.
///
/// Idempotent: calling this while a transaction is already open returns
/// `true` without re-entering Detours.
pub fn begin_transaction() -> bool {
    #[cfg(windows)]
    {
        if IN_TRANSACTION.with(|c| c.get()) {
            return true;
        }
        let status = unsafe { ffi::DetourTransactionBegin() };
        if status != ffi::NO_ERROR {
            LOG_PLATFORM_WARN!("DetourTransactionBegin failed (status={})", status);
            return false;
        }
        IN_TRANSACTION.with(|c| c.set(true));
        THREAD_ENROLLED.with(|c| c.set(false));
        true
    }
    #[cfg(not(windows))]
    {
        LOG_PLATFORM_WARN!("detour::begin_transaction: unsupported on this target");
        false
    }
}

/// Commit a detour transaction.
///
/// Every `attach` / `detach` registered since [`begin_transaction`]
/// applies atomically. Returns `false` if no transaction is open on
/// this thread, or if Detours refuses the commit.
pub fn commit_transaction() -> bool {
    #[cfg(windows)]
    {
        if !IN_TRANSACTION.with(|c| c.get()) {
            LOG_PLATFORM_WARN!("detour::commit_transaction called with no transaction open");
            return false;
        }
        let status = unsafe { ffi::DetourTransactionCommit() };
        IN_TRANSACTION.with(|c| c.set(false));
        THREAD_ENROLLED.with(|c| c.set(false));
        if status != ffi::NO_ERROR {
            LOG_PLATFORM_WARN!("DetourTransactionCommit failed (status={})", status);
            return false;
        }
        true
    }
    #[cfg(not(windows))]
    {
        LOG_PLATFORM_WARN!("detour::commit_transaction: unsupported on this target");
        false
    }
}

/// Abort a detour transaction, undoing any pending attaches.
///
/// Not part of the C++ public API, but present here for tests and for
/// the hook layer to use when an attach fails mid-sequence: the C++
/// `HookMacros.h` sets `_ost_detour_transaction_ok_ = false` and
/// expects the surrounding code to unwind, which maps cleanly onto an
/// explicit abort.
pub fn abort_transaction() -> bool {
    #[cfg(windows)]
    {
        if !IN_TRANSACTION.with(|c| c.get()) {
            return false;
        }
        let status = unsafe { ffi::DetourTransactionAbort() };
        IN_TRANSACTION.with(|c| c.set(false));
        THREAD_ENROLLED.with(|c| c.set(false));
        status == ffi::NO_ERROR
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Install a detour.
///
/// # Safety
///
/// * `target` must point to a writable `*mut c_void` slot owned by the
///   caller. Detours writes the trampoline address into it.
/// * `detour` must be a valid code address with a signature compatible
///   with the target.
/// * A transaction must be open on the calling thread.
/// * Every successful `attach` must be paired with a matching `detach`
///   before the surrounding code unloads the module that owns `detour`.
pub unsafe fn attach(target: *mut *mut c_void, detour: *mut c_void) -> bool {
    #[cfg(windows)]
    {
        if target.is_null() || detour.is_null() {
            LOG_PLATFORM_WARN!("detour::attach called with null target/detour");
            return false;
        }
        if !IN_TRANSACTION.with(|c| c.get()) {
            LOG_PLATFORM_WARN!("detour::attach called outside a transaction");
            return false;
        }
        if !enrol_current_thread() {
            return false;
        }
        let status = ffi::DetourAttach(target, detour);
        if status != ffi::NO_ERROR {
            LOG_PLATFORM_WARN!("DetourAttach failed (status={}, target={:p})", status, target);
            return false;
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = (target, detour);
        false
    }
}

/// Remove a detour installed by [`attach`].
///
/// # Safety
///
/// `target` and `detour` must be the exact pair passed to [`attach`].
/// A transaction must be open on the calling thread.
pub unsafe fn detach(target: *mut *mut c_void, detour: *mut c_void) -> bool {
    #[cfg(windows)]
    {
        if target.is_null() || detour.is_null() {
            LOG_PLATFORM_WARN!("detour::detach called with null target/detour");
            return false;
        }
        if !IN_TRANSACTION.with(|c| c.get()) {
            LOG_PLATFORM_WARN!("detour::detach called outside a transaction");
            return false;
        }
        if !enrol_current_thread() {
            return false;
        }
        let status = ffi::DetourDetach(target, detour);
        if status != ffi::NO_ERROR {
            LOG_PLATFORM_WARN!("DetourDetach failed (status={}, target={:p})", status, target);
            return false;
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = (target, detour);
        false
    }
}

#[cfg(windows)]
fn enrol_current_thread() -> bool {
    if THREAD_ENROLLED.with(|c| c.get()) {
        return true;
    }
    let h = crate::platform::thread::current_native_thread_handle();
    let status = unsafe { ffi::DetourUpdateThread(h) };
    if status != ffi::NO_ERROR {
        LOG_PLATFORM_WARN!("DetourUpdateThread failed (status={})", status);
        return false;
    }
    THREAD_ENROLLED.with(|c| c.set(true));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Detours transactions are per-thread. The Rust test harness runs
    // each test on its own thread, so the thread-local state is clean
    // at entry. A serial mutex guards the tests that touch Detours
    // itself, because some Detours calls assume process-wide state.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn reset_thread_state() {
        IN_TRANSACTION.with(|c| c.set(false));
        THREAD_ENROLLED.with(|c| c.set(false));
    }

    #[test]
    fn commit_without_begin_fails() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        assert!(!commit_transaction());
    }

    #[test]
    fn abort_without_begin_fails() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        assert!(!abort_transaction());
    }

    #[cfg(windows)]
    #[test]
    fn begin_commit_roundtrip() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        assert!(begin_transaction());
        assert!(IN_TRANSACTION.with(|c| c.get()));
        assert!(commit_transaction());
        assert!(!IN_TRANSACTION.with(|c| c.get()));
    }

    #[cfg(windows)]
    #[test]
    fn begin_is_idempotent() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        assert!(begin_transaction());
        assert!(begin_transaction());
        assert!(commit_transaction());
    }

    #[cfg(windows)]
    #[test]
    fn abort_clears_state() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        assert!(begin_transaction());
        assert!(abort_transaction());
        assert!(!IN_TRANSACTION.with(|c| c.get()));
    }

    #[cfg(windows)]
    #[test]
    fn commit_after_commit_fails() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        assert!(begin_transaction());
        assert!(commit_transaction());
        // No transaction is open now.
        assert!(!commit_transaction());
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_begin_fails() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        assert!(!begin_transaction());
    }

    #[test]
    fn attach_with_null_target_returns_false() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        let ok = unsafe { attach(std::ptr::null_mut(), 0x1234 as *mut c_void) };
        assert!(!ok);
    }

    #[test]
    fn attach_with_null_detour_returns_false() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        let mut target: *mut c_void = 0x1000 as *mut c_void;
        let ok = unsafe { attach(&mut target, std::ptr::null_mut()) };
        assert!(!ok);
    }

    #[test]
    fn attach_outside_transaction_returns_false() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        let mut target: *mut c_void = 0x1000 as *mut c_void;
        let ok = unsafe { attach(&mut target, 0x2000 as *mut c_void) };
        assert!(!ok);
    }

    #[test]
    fn detach_outside_transaction_returns_false() {
        let _g = SERIAL.lock().unwrap();
        reset_thread_state();
        let mut target: *mut c_void = 0x1000 as *mut c_void;
        let ok = unsafe { detach(&mut target, 0x2000 as *mut c_void) };
        assert!(!ok);
    }
}
