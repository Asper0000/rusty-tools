//! Hook boilerplate macros. Ported from `HookMacros.h` and the
//! `CAPTURE_THIS_FUNC` / `ARM_*` family in `VehCommon.h`.
//!
//! Rust has no `__fastcall`; on x64 the Microsoft ABI is the default, so
//! `extern "system"` is used throughout, matching the C++ on this target.
//!
//! ## The `Slot` newtype
//!
//! The C++ `o##name` variable is written by Detours' `DetourAttach`
//! (which stores the trampoline address into it) and read by both the
//! hook body and Detours itself. Modelling it as a Rust `AtomicPtr`
//! would be UB in the strict sense: Detours writes through the raw
//! pointer, not through Rust's atomic API. A `Slot` wraps an
//! `UnsafeCell<*mut c_void>` so the FFI write and the Rust reads share
//! the same memory without pretending the access is atomic.

use std::cell::UnsafeCell;
use std::ffi::c_void;

/// A pointer-sized slot that Detours writes a trampoline into.
pub struct Slot(UnsafeCell<*mut c_void>);

// SAFETY: Detours guarantees writes only happen while every other thread
// is suspended (its commit path suspends the process). Reads happen
// either from the same init thread or, once installed, from any thread
// invoking the trampoline. The slot is process-global; no thread owns it.
unsafe impl Send for Slot {}
unsafe impl Sync for Slot {}

impl Slot {
    pub const fn new() -> Self { Self(UnsafeCell::new(std::ptr::null_mut())) }

    /// Raw pointer for Detours: the address of the slot itself, which is
    /// what `DetourAttach` writes the trampoline into.
    #[inline]
    pub fn as_ptr(&self) -> *mut *mut c_void { self.0.get() }

    /// # Safety
    /// Reads the raw pointer without synchronisation. Callers must
    /// ensure no concurrent Detours write is in flight.
    #[inline]
    pub unsafe fn load(&self) -> *mut c_void { *self.0.get() }

    /// # Safety
    /// Writes the raw pointer. Callers must ensure no concurrent reader
    /// relies on the old value.
    #[inline]
    pub unsafe fn store(&self, p: *mut c_void) { *self.0.get() = p; }

    /// # Safety
    /// Same as [`load`].
    #[inline]
    pub unsafe fn is_null(&self) -> bool { (*self.0.get()).is_null() }
}

impl Default for Slot {
    fn default() -> Self { Self::new() }
}

/// Define a hooked function.
///
/// Expands to a `Slot` holding the original fn pointer, a `o_<Name>`
/// helper that calls through the slot, and a `hk_<Name>` `extern "system"`
/// function the caller writes the body for.
#[macro_export]
macro_rules! hook_fn {
    (
        $(#[$meta:meta])*
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty $body:block
    ) => {
        ::paste::paste! {
            #[allow(non_upper_case_globals)]
            pub static [<O_ $name:snake:upper>]: $crate::hook::macros::Slot
                = $crate::hook::macros::Slot::new();

            #[allow(non_snake_case)]
            #[inline(always)]
            pub unsafe fn [<o_ $name>]($($arg : $ty),*) -> $ret {
                let p = [<O_ $name:snake:upper>].load();
                debug_assert!(
                    !p.is_null(),
                    concat!("hook ", stringify!($name), " invoked before install"),
                );
                let f: unsafe extern "system" fn($($ty),*) -> $ret
                    = ::core::mem::transmute(p);
                f($($arg),*)
            }

            $(#[$meta])*
            #[allow(non_snake_case)]
            pub unsafe extern "system" fn [<hk_ $name>]($($arg : $ty),*) -> $ret $body
        }
    };
}

/// Define a passthrough function that is resolved by pattern but not
/// hooked. Expands to a `Slot` plus an `o_<Name>` helper.
#[macro_export]
macro_rules! resolve_fn {
    (
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty
    ) => {
        ::paste::paste! {
            #[allow(non_upper_case_globals)]
            pub static [<O_ $name:snake:upper>]: $crate::hook::macros::Slot
                = $crate::hook::macros::Slot::new();

            #[allow(non_snake_case)]
            #[inline(always)]
            pub unsafe fn [<o_ $name>]($($arg : $ty),*) -> $ret {
                let p = [<O_ $name:snake:upper>].load();
                debug_assert!(
                    !p.is_null(),
                    concat!("resolve_fn ", stringify!($name), " invoked before resolve"),
                );
                let f: unsafe extern "system" fn($($ty),*) -> $ret
                    = ::core::mem::transmute(p);
                f($($arg),*)
            }
        }
    };
}

/// Declares a function whose `this` pointer is captured once via a
/// one-shot int3 (C++ `CAPTURE_THIS_FUNC`). Expands to two slots — one
/// for the resolved function address, one for the captured `this` — and
/// the same `o_<Name>` helper as `resolve_fn!`.
#[macro_export]
macro_rules! capture_this_fn {
    (
        fn $name:ident($($arg:ident : $ty:ty),* $(,)?) -> $ret:ty
    ) => {
        ::paste::paste! {
            #[allow(non_upper_case_globals)]
            pub static [<O_ $name:snake:upper>]: $crate::hook::macros::Slot
                = $crate::hook::macros::Slot::new();

            #[allow(non_upper_case_globals)]
            pub static [<CAPTURED_ $name:snake:upper>]: $crate::hook::macros::Slot
                = $crate::hook::macros::Slot::new();

            #[allow(non_snake_case)]
            #[inline(always)]
            pub unsafe fn [<o_ $name>]($($arg : $ty),*) -> $ret {
                let p = [<O_ $name:snake:upper>].load();
                debug_assert!(
                    !p.is_null(),
                    concat!("capture_this_fn ", stringify!($name), " invoked before arm"),
                );
                let f: unsafe extern "system" fn($($ty),*) -> $ret
                    = ::core::mem::transmute(p);
                f($($arg),*)
            }
        }
    };
}

/// Install a hook in the given module. Returns `true` when the hook is
/// (or already was) live, `false` when Detours refused. A missing
/// pattern is *not* a failure here: `pattern_loader` records it in the
/// missing list, and the C++ behaves the same way.
#[macro_export]
macro_rules! __install_hook_step {
    ($hmod:expr, $name:ident) => {{
        let target = $crate::utils::steam_metadata::pattern_loader::find_pattern(
            $hmod, stringify!($name),
        );
        if target.is_null() {
            true
        } else {
            ::paste::paste! {
                unsafe { [<O_ $name:snake:upper>].store(target); }
                let slot = [<O_ $name:snake:upper>].as_ptr();
                let detour: *mut ::core::ffi::c_void =
                    [<hk_ $name>] as *const () as usize as *mut ::core::ffi::c_void;
                // SAFETY: slot is a process-static `*mut *mut c_void`;
                // detour is our own `hk_` function, whose signature
                // matches the resolved target on x64. Caller is inside a
                // Detours transaction.
                unsafe { $crate::platform::detour::attach(slot, detour) }
            }
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __install_hook_c {
    ($name:ident) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::client_hmodule(),
        );
        $crate::__install_hook_step!(hmod, $name)
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __install_hook_u {
    ($name:ident) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::ui_hmodule(),
        );
        $crate::__install_hook_step!(hmod, $name)
    }};
}

#[macro_export]
macro_rules! install_hook_c { ($name:ident) => { $crate::__install_hook_c!($name) }; }
#[macro_export]
macro_rules! install_hook_u { ($name:ident) => { $crate::__install_hook_u!($name) }; }

/// Uninstall a hook installed by `install_hook_c!` / `install_hook_u!`.
/// Clears the slot on success, matching the C++ `o##name = nullptr`.
#[macro_export]
macro_rules! uninstall_hook {
    ($name:ident) => {{
        ::paste::paste! {
            let p = unsafe { [<O_ $name:snake:upper>].load() };
            if p.is_null() {
                true
            } else {
                let slot = [<O_ $name:snake:upper>].as_ptr();
                let detour: *mut ::core::ffi::c_void =
                    [<hk_ $name>] as *const () as usize as *mut ::core::ffi::c_void;
                let ok = unsafe { $crate::platform::detour::detach(slot, detour) };
                if ok {
                    unsafe { [<O_ $name:snake:upper>].store(::core::ptr::null_mut()); }
                }
                ok
            }
        }
    }};
}

/// Resolve `name` in the given module without hooking. Stores the
/// address in the `O_<NAME>` slot declared by `resolve_fn!` or
/// `capture_this_fn!`.
#[macro_export]
#[doc(hidden)]
macro_rules! __resolve_step {
    ($hmod:expr, $name:ident) => {{
        let target = $crate::utils::steam_metadata::pattern_loader::find_pattern(
            $hmod, stringify!($name),
        );
        if !target.is_null() {
            ::paste::paste! {
                unsafe { [<O_ $name:snake:upper>].store(target); }
            }
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __resolve_c {
    ($name:ident) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::client_hmodule(),
        );
        $crate::__resolve_step!(hmod, $name)
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __resolve_u {
    ($name:ident) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::ui_hmodule(),
        );
        $crate::__resolve_step!(hmod, $name)
    }};
}

#[macro_export]
macro_rules! resolve_c { ($name:ident) => { $crate::__resolve_c!($name) }; }
#[macro_export]
macro_rules! resolve_u { ($name:ident) => { $crate::__resolve_u!($name) }; }

/// Arm a one-shot int3 capture site that writes RCX (the implicit
/// `this` on x64) into the `CAPTURED_<NAME>` slot declared by
/// `capture_this_fn!`.
#[macro_export]
#[doc(hidden)]
macro_rules! __arm_capture_step {
    ($hmod:expr, $name:ident) => {{
        let target = $crate::utils::steam_metadata::pattern_loader::find_pattern(
            $hmod, stringify!($name),
        );
        if !target.is_null() {
            ::paste::paste! {
                unsafe { [<O_ $name:snake:upper>].store(target); }
                // SAFETY: target points at the first byte of a function
                // inside a loaded module; the module is mapped for the
                // lifetime of the process.
                let original = unsafe { *(target as *const u8) };
                $crate::hook::veh_common::arm($crate::hook::veh_common::Int3Site {
                    target: target as *mut u8,
                    original_byte: original,
                    persistent: false,
                    on_hit: $crate::hook::veh_common::capture_rcx_to,
                    callback_data: [<CAPTURED_ $name:snake:upper>].as_ptr()
                        as *mut ::core::ffi::c_void,
                    label: stringify!($name),
                });
            }
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __arm_capture_c {
    ($name:ident) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::client_hmodule(),
        );
        $crate::__arm_capture_step!(hmod, $name)
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __arm_capture_u {
    ($name:ident) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::ui_hmodule(),
        );
        $crate::__arm_capture_step!(hmod, $name)
    }};
}

#[macro_export]
macro_rules! arm_capture_c { ($name:ident) => { $crate::__arm_capture_c!($name) }; }
#[macro_export]
macro_rules! arm_capture_u { ($name:ident) => { $crate::__arm_capture_u!($name) }; }

/// `true` when both the captured `this` slot and the resolved function
/// slot are populated. Expands to a `&&` of two reads; the caller must
/// be in the module where `capture_this_fn!` declared the slots.
#[macro_export]
macro_rules! capture_ready {
    ($name:ident) => {
        ::paste::paste! {
            unsafe {
                ![<CAPTURED_ $name:snake:upper>].is_null()
                    && ![<O_ $name:snake:upper>].is_null()
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    #[test]
    fn slot_starts_null() {
        let s = Slot::new();
        assert!(unsafe { s.is_null() });
    }

    #[test]
    fn slot_round_trip() {
        let s = Slot::new();
        let p = 0x1234usize as *mut c_void;
        unsafe {
            s.store(p);
            assert_eq!(s.load(), p);
            assert!(!s.is_null());
        }
    }

    #[test]
    fn slot_as_ptr_addresses_the_storage() {
        let s = Slot::new();
        let p = 0x5678usize as *mut c_void;
        unsafe {
            s.store(p);
            let slot = s.as_ptr();
            assert_eq!(*slot, p);
        }
    }
}

// ── arm_int3_c! / arm_int3_u! ───────────────────────────────────────
// Register a persistent or one-shot int3 site whose callback is a
// custom function. Mirrors `ARM_INT3` in `VehCommon.h`. Unlike
// `arm_capture_*!`, this does not touch a Slot: the site has no
// "call the original" path, only an on-hit callback that may inspect
// and mutate the CPU context.

#[macro_export]
#[doc(hidden)]
macro_rules! __arm_int3_step {
    ($hmod:expr, $name:ident, $persistent:expr, $on_hit:path, $callback_data:expr) => {{
        let target = $crate::utils::steam_metadata::pattern_loader::find_pattern(
            $hmod, stringify!($name),
        );
        if !target.is_null() {
            // SAFETY: target points at the first byte of a function inside
            // a mapped module. The module is mapped for the process
            // lifetime, so reading that byte is safe.
            let original = unsafe { *(target as *const u8) };
            $crate::hook::veh_common::arm($crate::hook::veh_common::Int3Site {
                target: target as *mut u8,
                original_byte: original,
                persistent: $persistent,
                on_hit: $on_hit,
                callback_data: $callback_data,
                label: stringify!($name),
            });
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __arm_int3_c {
    ($name:ident, $persistent:expr, $on_hit:path, $callback_data:expr) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::client_hmodule(),
        );
        $crate::__arm_int3_step!(hmod, $name, $persistent, $on_hit, $callback_data)
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __arm_int3_u {
    ($name:ident, $persistent:expr, $on_hit:path, $callback_data:expr) => {{
        let hmod = $crate::platform::dynamic_library::ModuleHandle(
            $crate::dllmain::ui_hmodule(),
        );
        $crate::__arm_int3_step!(hmod, $name, $persistent, $on_hit, $callback_data)
    }};
}

#[macro_export]
macro_rules! arm_int3_c {
    ($name:ident, $persistent:expr, $on_hit:path, $callback_data:expr) => {
        $crate::__arm_int3_c!($name, $persistent, $on_hit, $callback_data)
    };
}

#[macro_export]
macro_rules! arm_int3_u {
    ($name:ident, $persistent:expr, $on_hit:path, $callback_data:expr) => {
        $crate::__arm_int3_u!($name, $persistent, $on_hit, $callback_data)
    };
}


/// `true` when the `O_<NAME>` slot declared by `resolve_fn!` is
/// populated. Companion to `capture_ready!`, which requires both the
/// captured `this` and the resolved address.
#[macro_export]
macro_rules! resolve_ready {
    ($name:ident) => {
        ::paste::paste! {
            unsafe { ![<O_ $name:snake:upper>].is_null() }
        }
    };
}

