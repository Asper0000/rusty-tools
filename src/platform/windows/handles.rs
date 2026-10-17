//! RAII wrappers for Win32 resources. Source: `OSTPlatform/Windows/Handles.h`

use std::ffi::c_void;
use std::marker::PhantomData;

pub type HANDLE = *mut c_void;
pub type HKEY   = *mut c_void;

/// `(HANDLE)-1`. The canonical "invalid" handle value returned by some
/// Win32 APIs (notably `CreateFileW`).
pub const INVALID_HANDLE_VALUE: HANDLE = !0usize as HANDLE;

// ─── HandlePolicy ───────────────────────────────────────────────────

pub trait HandlePolicy: Copy + Clone {
    fn is_valid(h: HANDLE) -> bool;
    fn close(h: HANDLE);
}

#[derive(Copy, Clone, Debug, Default)]
pub struct NullHandlePolicy;

impl HandlePolicy for NullHandlePolicy {
    fn is_valid(h: HANDLE) -> bool { !h.is_null() }
    fn close(h: HANDLE) {
        #[cfg(windows)]
        unsafe { windows_sys::Win32::Foundation::CloseHandle(h); }
        #[cfg(not(windows))]
        { let _ = h; }
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub struct InvalidHandlePolicy;

impl HandlePolicy for InvalidHandlePolicy {
    fn is_valid(h: HANDLE) -> bool {
        !h.is_null() && h != INVALID_HANDLE_VALUE
    }
    fn close(h: HANDLE) {
        #[cfg(windows)]
        unsafe { windows_sys::Win32::Foundation::CloseHandle(h); }
        #[cfg(not(windows))]
        { let _ = h; }
    }
}

// ─── UniqueHandleT<P> ───────────────────────────────────────────────

pub struct UniqueHandleT<P: HandlePolicy> {
    handle: HANDLE,
    _policy: PhantomData<P>,
}

impl<P: HandlePolicy> UniqueHandleT<P> {
    pub const fn new() -> Self {
        Self { handle: std::ptr::null_mut(), _policy: PhantomData }
    }

    pub const fn from_raw(handle: HANDLE) -> Self {
        Self { handle, _policy: PhantomData }
    }

    pub fn get(&self) -> HANDLE { self.handle }

    pub fn is_valid(&self) -> bool { P::is_valid(self.handle) }

    pub fn reset_to(&mut self, handle: HANDLE) {
        if P::is_valid(self.handle) {
            P::close(self.handle);
        }
        self.handle = handle;
    }

    pub fn reset(&mut self) {
        self.reset_to(std::ptr::null_mut());
    }
}

impl<P: HandlePolicy> Default for UniqueHandleT<P> {
    fn default() -> Self { Self::new() }
}

impl<P: HandlePolicy> Drop for UniqueHandleT<P> {
    fn drop(&mut self) {
        if P::is_valid(self.handle) {
            P::close(self.handle);
        }
    }
}

pub type UniqueHandle     = UniqueHandleT<NullHandlePolicy>;
pub type UniqueFileHandle = UniqueHandleT<InvalidHandlePolicy>;

// ─── UniqueRegKey ───────────────────────────────────────────────────

pub struct UniqueRegKey {
    key: HKEY,
}

impl UniqueRegKey {
    pub const fn new() -> Self {
        Self { key: std::ptr::null_mut() }
    }

    pub fn get(&self) -> HKEY { self.key }
    pub fn is_valid(&self) -> bool { !self.key.is_null() }

    pub fn put(&mut self) -> *mut HKEY {
        self.reset();
        &mut self.key
    }

    pub fn reset(&mut self) {
        if !self.key.is_null() {
            #[cfg(windows)]
            unsafe { windows_sys::Win32::System::Registry::RegCloseKey(self.key); }
        }
        self.key = std::ptr::null_mut();
    }
}

impl Default for UniqueRegKey {
    fn default() -> Self { Self::new() }
}

impl Drop for UniqueRegKey {
    fn drop(&mut self) {
        self.reset();
    }
}

// ─── UniqueMapView ──────────────────────────────────────────────────

pub struct UniqueMapView {
    base: *const u8,
}

impl UniqueMapView {
    pub const fn new() -> Self {
        Self { base: std::ptr::null() }
    }

    pub const fn from_raw(base: *const u8) -> Self {
        Self { base }
    }

    pub fn get(&self) -> *const u8 { self.base }
    pub fn is_valid(&self) -> bool { !self.base.is_null() }

    pub fn reset(&mut self) {
        if !self.base.is_null() {
            #[cfg(windows)]
            unsafe {
                // UnmapViewOfFile takes a MEMORY_MAPPED_VIEW_ADDRESS wrapper
                // struct in windows-sys 0.59. The field is a *mut c_void.
                let addr = windows_sys::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.base as *mut c_void,
                };
                windows_sys::Win32::System::Memory::UnmapViewOfFile(addr);
            }
        }
        self.base = std::ptr::null();
    }
}

impl Default for UniqueMapView {
    fn default() -> Self { Self::new() }
}

impl Drop for UniqueMapView {
    fn drop(&mut self) {
        self.reset();
    }
}

// ─── ScopeExit<F> ───────────────────────────────────────────────────

pub struct ScopeExit<F: FnOnce()> {
    closure: Option<F>,
}

impl<F: FnOnce()> ScopeExit<F> {
    pub fn new(closure: F) -> Self {
        Self { closure: Some(closure) }
    }

    pub fn dismiss(&mut self) {
        self.closure = None;
    }
}

impl<F: FnOnce()> Drop for ScopeExit<F> {
    fn drop(&mut self) {
        if let Some(f) = self.closure.take() {
            f();
        }
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    thread_local! {
        static CLOSE_COUNT: Cell<u32> = const { Cell::new(0) };
    }

    fn reset_count() { CLOSE_COUNT.with(|c| c.set(0)); }
    fn count()       -> u32 { CLOSE_COUNT.with(|c| c.get()) }

    #[derive(Copy, Clone)]
    struct SpyPolicy;

    impl HandlePolicy for SpyPolicy {
        fn is_valid(h: HANDLE) -> bool { !h.is_null() }
        fn close(_h: HANDLE) { CLOSE_COUNT.with(|c| c.set(c.get() + 1)); }
    }

    #[test]
    fn null_handle_default_is_empty() {
        let h = UniqueHandle::new();
        assert!(h.get().is_null());
        assert!(!h.is_valid());
    }

    #[test]
    fn invalid_handle_value_is_not_valid() {
        let h = UniqueFileHandle::from_raw(INVALID_HANDLE_VALUE);
        assert!(!h.is_valid());
    }

    #[test]
    fn null_handle_policy_accepts_null() {
        assert!(!NullHandlePolicy::is_valid(std::ptr::null_mut()));
        assert!(NullHandlePolicy::is_valid(0x1000 as HANDLE));
    }

    #[test]
    fn invalid_handle_policy_rejects_invalid_value() {
        assert!(!InvalidHandlePolicy::is_valid(INVALID_HANDLE_VALUE));
        assert!(!InvalidHandlePolicy::is_valid(std::ptr::null_mut()));
        assert!(InvalidHandlePolicy::is_valid(0x1000 as HANDLE));
    }

    #[test]
    fn spy_policy_close_runs_on_drop() {
        reset_count();
        {
            let _h = UniqueHandleT::<SpyPolicy>::from_raw(0x1000 as HANDLE);
        }
        assert_eq!(count(), 1);
    }

    #[test]
    fn spy_policy_close_skipped_for_null() {
        reset_count();
        {
            let _h = UniqueHandleT::<SpyPolicy>::from_raw(std::ptr::null_mut());
        }
        assert_eq!(count(), 0);
    }

    #[test]
    fn spy_policy_close_runs_on_reset_to() {
        reset_count();
        let mut h = UniqueHandleT::<SpyPolicy>::from_raw(0x1000 as HANDLE);
        h.reset_to(0x2000 as HANDLE);
        assert_eq!(count(), 1);
        drop(h);
        assert_eq!(count(), 2);
    }

    #[test]
    fn spy_policy_close_runs_once_on_double_reset() {
        reset_count();
        let mut h = UniqueHandleT::<SpyPolicy>::from_raw(0x1000 as HANDLE);
        h.reset();
        h.reset();
        drop(h);
        assert_eq!(count(), 1);
    }

    #[test]
    fn spy_policy_metadata_round_trips() {
        let h = UniqueHandleT::<SpyPolicy>::from_raw(0x1234 as HANDLE);
        assert_eq!(h.get() as usize, 0x1234);
        assert!(h.is_valid());
        std::mem::forget(h);
    }

    #[test]
    fn reg_key_default_is_empty() {
        let k = UniqueRegKey::new();
        assert!(k.get().is_null());
        assert!(!k.is_valid());
    }

    #[test]
    fn map_view_default_is_empty() {
        let m = UniqueMapView::new();
        assert!(m.get().is_null());
        assert!(!m.is_valid());
    }

    #[test]
    fn map_view_metadata_round_trips() {
        let m = UniqueMapView::from_raw(0x8000 as *const u8);
        assert_eq!(m.get() as usize, 0x8000);
        assert!(m.is_valid());
        std::mem::forget(m);
    }

    #[test]
    fn scope_exit_runs_on_drop() {
        let fired = Cell::new(false);
        {
            let _g = ScopeExit::new(|| { fired.set(true); });
        }
        assert!(fired.get());
    }

    #[test]
    fn scope_exit_dismiss_prevents_run() {
        let fired = Cell::new(false);
        {
            let mut g = ScopeExit::new(|| { fired.set(true); });
            g.dismiss();
        }
        assert!(!fired.get());
    }

    #[test]
    fn scope_exit_runs_exactly_once() {
        let counter = Cell::new(0u32);
        {
            let _g = ScopeExit::new(|| { counter.set(counter.get() + 1); });
        }
        assert_eq!(counter.get(), 1);
    }
}
