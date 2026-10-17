//! Raw FFI to the Microsoft Detours C API.
//!
//! Linked against the vendored `third_party/Detours` static library.
//! `build.rs` emits `cargo:rustc-link-search=native=.../lib.X64` and
//! `cargo:rustc-link-lib=static=detours`; this module only declares the
//! symbols.
//!
//! ## Calling convention
//!
//! Detours' prototypes are `WINAPI`, i.e. `__stdcall` on x86 and the
//! platform ABI on x64. `extern "system"` is correct on both.
//!
//! ## Contract
//!
//! Every `DetourAttach` / `DetourDetach` must happen between a
//! `DetourTransactionBegin` and a `DetourTransactionCommit` (or
//! `DetourTransactionAbort`) on the same thread.
//!
//! `DetourUpdateThread` enrols a thread's context in the transaction so
//! its instruction pointer is fixed up if it happens to be inside the
//! prolog of a hooked function. On x64, commit suspends the process and
//! does this automatically for every thread other than the caller; the
//! calling thread must be enrolled explicitly before the first attach
//! or detach in the transaction.

#![allow(non_snake_case)]

use std::ffi::c_void;

/// Detours' status type: `LONG`. `NO_ERROR` (0) means success.
pub type LONG = i32;
pub const NO_ERROR: LONG = 0;

extern "system" {
    /// Begin a transaction on the calling thread.
    pub fn DetourTransactionBegin() -> LONG;

    /// Abort a transaction, undoing every attach/detach registered in it.
    pub fn DetourTransactionAbort() -> LONG;

    /// Apply every attach/detach registered in the transaction atomically.
    /// Suspends the process, updates other threads, then patches.
    pub fn DetourTransactionCommit() -> LONG;

    /// Enrol a thread's context in the transaction. `thread` is a
    /// `HANDLE` as returned by `GetCurrentThread` or `OpenThread`.
    pub fn DetourUpdateThread(thread: *mut c_void) -> LONG;

    /// Attach a detour.
    ///
    /// `target` points to the caller's copy of the function-pointer
    /// slot. Detours overwrites `*target` with the trampoline address
    /// and patches the target function in place.
    pub fn DetourAttach(target: *mut *mut c_void, detour: *mut c_void) -> LONG;

    /// Remove a detour installed by `DetourAttach` with the same pair.
    pub fn DetourDetach(target: *mut *mut c_void, detour: *mut c_void) -> LONG;

    /// Restore the Detours runtime state after a DLL has been loaded.
    /// Called once from the injected DLL after `DllMain`.
    pub fn DetourRestoreAfterWith() -> LONG;
}
