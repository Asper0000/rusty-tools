//! Vectored-exception-handler support. Source: `OSTPlatform/Windows/Trap.cpp`
//!
//! Real implementation on Windows. `AddVectoredExceptionHandler` is a
//! Win32 call; the CONTEXT struct from `windows-sys` gives typed access
//! to RCX/RDX/R8/R9/RSP/RIP/EFlags.

use std::ffi::c_void;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExceptionKind {
    Breakpoint,
    SingleStep,
    Other,
}

/// The x64 register context at the point of an exception.
pub struct Context {
    native: *mut c_void,
}

impl Context {
    /// # Safety
    ///
    /// `ptr` must be a valid `CONTEXT*` that stays live for the duration
    /// of any use of the returned `Context`.
    pub unsafe fn from_native(ptr: *mut c_void) -> Self {
        Self { native: ptr }
    }

    pub fn instruction_pointer(&self) -> u64 {
        #[cfg(windows)]
        {
            if self.native.is_null() { return 0; }
            unsafe {
                let ctx = self.native as *const windows_sys::Win32::System::Diagnostics::Debug::CONTEXT;
                (*ctx).Rip
            }
        }
        #[cfg(not(windows))]
        { 0 }
    }

    /// Nth (1-based) integer/pointer argument under the x64 fastcall
    /// convention. Args 1-4 from RCX/RDX/R8/R9; 5+ from the stack.
    /// Only valid when the trap fired at the function's first instruction.
    pub fn argument(&self, index: i32) -> u64 {
        #[cfg(windows)]
        {
            if self.native.is_null() || index <= 0 { return 0; }
            unsafe {
                let ctx = self.native as *const windows_sys::Win32::System::Diagnostics::Debug::CONTEXT;
                match index {
                    1 => (*ctx).Rcx,
                    2 => (*ctx).Rdx,
                    3 => (*ctx).R8,
                    4 => (*ctx).R9,
                    _ => {
                        // Stack slot: [RSP + 8*index]. The C++ computes
                        // RSP + index*8, and reads a u64 from there.
                        let slot = (*ctx).Rsp.wrapping_add((index as u64) * 8);
                        std::ptr::read_unaligned(slot as *const u64)
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = index;
            0
        }
    }

    pub fn enable_single_step(&mut self) {
        #[cfg(windows)]
        {
            if self.native.is_null() { return; }
            unsafe {
                let ctx = self.native as *mut windows_sys::Win32::System::Diagnostics::Debug::CONTEXT;
                (*ctx).EFlags |= 0x100;
            }
        }
    }
}

/// VEH handler installed by the caller. Returning `true` requests
/// `EXCEPTION_CONTINUE_EXECUTION`; `false` lets the exception propagate.
pub type Handler = fn(ExceptionKind, &mut Context) -> bool;

/// Opaque handle to an installed handler.
pub type HandlerHandle = *mut c_void;

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::atomic::{AtomicPtr, Ordering};
    // Both exception codes are exported from Win32::Foundation.
    use windows_sys::Win32::Foundation::{EXCEPTION_BREAKPOINT, EXCEPTION_SINGLE_STEP};
    use windows_sys::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, RemoveVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    // The C++ installs exactly one global handler at a time; do the same.
    static HANDLER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

    unsafe extern "system" fn veh_thunk(info: *mut EXCEPTION_POINTERS) -> i32 {
        let h = HANDLER.load(Ordering::Acquire);
        if h.is_null() { return 0; }
        if info.is_null() { return 0; }

        let rec = (*info).ExceptionRecord;
        let ctx = (*info).ContextRecord;
        if rec.is_null() || ctx.is_null() { return 0; }

        let code = (*rec).ExceptionCode;
        let kind = if code == EXCEPTION_BREAKPOINT {
            ExceptionKind::Breakpoint
        } else if code == EXCEPTION_SINGLE_STEP {
            ExceptionKind::SingleStep
        } else {
            ExceptionKind::Other
        };

        let mut ctx_wrapper = unsafe { Context::from_native(ctx as *mut c_void) };
        let f: Handler = unsafe { std::mem::transmute(h) };
        let handled = f(kind, &mut ctx_wrapper);

        if handled { -1 } else { 0 }  // CONTINUE_EXECUTION / CONTINUE_SEARCH
    }

    pub(super) fn add(handler: Handler) -> HandlerHandle {
        let ptr = handler as *const () as *mut c_void;
        HANDLER.store(ptr, Ordering::Release);
        let h = unsafe { AddVectoredExceptionHandler(1, Some(veh_thunk)) };
        h as HandlerHandle
    }

    pub(super) fn remove(handle: HandlerHandle) {
        if handle.is_null() { return; }
        unsafe { RemoveVectoredExceptionHandler(handle as *mut c_void); }
        HANDLER.store(std::ptr::null_mut(), Ordering::Release);
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub(super) fn add(_handler: Handler) -> HandlerHandle { std::ptr::null_mut() }
    pub(super) fn remove(_handle: HandlerHandle) { }
}

pub fn add_vectored_handler(handler: Handler) -> HandlerHandle {
    imp::add(handler)
}

pub fn remove_vectored_handler(handle: HandlerHandle) {
    imp::remove(handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_from_null_is_usable() {
        let mut ctx = unsafe { Context::from_native(std::ptr::null_mut()) };
        assert_eq!(ctx.instruction_pointer(), 0);
        assert_eq!(ctx.argument(1), 0);
        assert_eq!(ctx.argument(5), 0);
        assert_eq!(ctx.argument(0), 0);
        assert_eq!(ctx.argument(-1), 0);
        ctx.enable_single_step();
    }

    #[test]
    fn exception_kind_equality() {
        assert_eq!(ExceptionKind::Breakpoint, ExceptionKind::Breakpoint);
        assert_ne!(ExceptionKind::Breakpoint, ExceptionKind::SingleStep);
    }

    #[cfg(not(windows))]
    #[test]
    fn add_handler_stub_returns_null() {
        fn noop(_: ExceptionKind, _: &mut Context) -> bool { false }
        let h = add_vectored_handler(noop);
        assert!(h.is_null());
        remove_vectored_handler(h);
    }

    #[cfg(windows)]
    #[test]
    fn install_and_remove_handler() {
        fn noop(_: ExceptionKind, _: &mut Context) -> bool { false }
        let h = add_vectored_handler(noop);
        assert!(!h.is_null(), "AddVectoredExceptionHandler returned null");
        remove_vectored_handler(h);
    }
}