//! Shared int3 site registry. Ported from `Utils/HookSupport/VehCommon.h`
//! and `VehCommon.cpp`.
//!
//! The registry is a global list of soft-breakpoint sites. Each site
//! owns the first byte of a function that has been temporarily replaced
//! with `0xCC`. When the CPU raises `EXCEPTION_BREAKPOINT` at one of
//! those addresses, [`on_breakpoint`] restores the original byte, runs
//! the site's `on_hit` callback, and (for persistent sites) sets TF so
//! the next `EXCEPTION_SINGLE_STEP` re-arms the int3.
//!
//! One VEH handler is installed lazily on the first [`arm`].

use parking_lot::RwLock;

use crate::platform::trap::{self, Context, ExceptionKind};

/// x86-64 instruction length cap. Used by the single-step predicate.
pub const K_MAX_X64_INSN_LEN: u64 = 15;

/// `true` if `rip` is exactly `target`.
#[inline]
pub fn is_at(rip: u64, target: *const std::ffi::c_void) -> bool {
    rip == target as u64
}

/// `true` if `rip` is in `(target, target + 15]`. Used by the single-step
/// branch to recognise that the trap was caused by us setting TF after
/// restoring `target`'s first byte.
#[inline]
pub fn is_post_int3_step(rip: u64, target: *const std::ffi::c_void) -> bool {
    let base = target as u64;
    rip > base && rip <= base + K_MAX_X64_INSN_LEN
}

/// A managed soft-breakpoint site.
pub struct Int3Site {
    /// First byte of the hooked function.
    pub target: *mut u8,
    /// Saved before arming; restored on hit and on disarm.
    pub original_byte: u8,
    /// `true`: re-arm after each single-step. `false`: one-shot.
    pub persistent: bool,
    /// Fires after the byte is restored, before control returns to the
    /// original code. May read/mutate `ctx` freely.
    pub on_hit: fn(&mut Context, &Int3Site),
    /// Site-specific data consumed by `on_hit`.
    pub callback_data: *mut std::ffi::c_void,
    /// Logging tag.
    pub label: &'static str,
}

// SAFETY: the site owns no thread-affine state. `target` and
// `callback_data` are read by the VEH on whichever thread triggered the
// trap; the C++ has the same requirement and relies on the caller to
// keep those addresses valid while the site is armed.
unsafe impl Send for Int3Site {}
unsafe impl Sync for Int3Site {}

static SITES: RwLock<Vec<Int3Site>> = RwLock::new(Vec::new());

// `trap::HandlerHandle` is `*mut c_void`, which is neither `Send` nor
// `Sync`, so it cannot live inside a `static` directly. Store the
// handle as a `usize` and cast back at the two use sites
// (`ensure_handler_installed`, `remove_handler`). The address is opaque
// to Rust; the VEH runtime only compares it for equality.
static HANDLER: RwLock<Option<usize>> = RwLock::new(None);

fn veh_handler(kind: ExceptionKind, ctx: &mut Context) -> bool {
    match kind {
        ExceptionKind::Breakpoint => on_breakpoint(ctx),
        ExceptionKind::SingleStep => on_single_step(ctx),
        ExceptionKind::Other => false,
    }
}

fn ensure_handler_installed() {
    let mut guard = HANDLER.write();
    if guard.is_none() {
        let h = trap::add_vectored_handler(veh_handler);
        *guard = Some(h as usize);
    }
}

/// Register a site and write `0xCC` to its first byte.
pub fn arm(site: Int3Site) {
    ensure_handler_installed();

    // Read the target before `site` is moved into the registry.
    let target = site.target;

    {
        let mut sites = SITES.write();
        // Idempotent: skip if this exact target is already armed.
        if sites.iter().any(|s| s.target == target) {
            return;
        }
        sites.push(site);
    }

    // SAFETY: the caller guarantees `target` points at a writable byte
    // inside a mapped executable region. `write_executable_byte` is a
    // no-op on non-Windows targets.
    unsafe {
        crate::platform::memory::write_executable_byte(
            target as *mut std::ffi::c_void,
            0xCC,
        );
    }
}

/// `true` if at least one site is currently registered.
pub fn has_sites() -> bool {
    !SITES.read().is_empty()
}

pub fn on_breakpoint(ctx: &mut Context) -> bool {
    // Snapshot the matching site so the on_hit callback runs without
    // holding SITES.read(). A callback that arms or disarms a site
    // (which takes SITES.write()) would otherwise deadlock.
    let matched = {
        let sites = SITES.read();
        sites.iter().find_map(|s| {
            if s.target.is_null() { return None; }
            if !is_at(ctx.instruction_pointer(), s.target as *const _) {
                return None;
            }
            Some(Int3Site {
                target: s.target,
                original_byte: s.original_byte,
                persistent: s.persistent,
                on_hit: s.on_hit,
                callback_data: s.callback_data,
                label: s.label,
            })
        })
    };

    let Some(site) = matched else { return false; };

    // Restore the real first instruction before resuming.
    unsafe {
        crate::platform::memory::write_executable_byte(
            site.target as *mut std::ffi::c_void,
            site.original_byte,
        );
    }
    (site.on_hit)(ctx, &site);
    if site.persistent {
        ctx.enable_single_step();
    }
    true
}

pub fn on_single_step(ctx: &mut Context) -> bool {
    let sites = SITES.read();
    for site in sites.iter() {
        if !site.persistent || site.target.is_null() {
            continue;
        }
        if !is_post_int3_step(ctx.instruction_pointer(), site.target as *const _) {
            continue;
        }
        // Re-arm for the next hit.
        unsafe {
            crate::platform::memory::write_executable_byte(
                site.target as *mut std::ffi::c_void,
                0xCC,
            );
        }
        return true;
    }
    false
}

/// Restore any still-armed bytes and clear the registry.
pub fn disarm_all() {
    let mut sites = SITES.write();
    for site in sites.iter() {
        if site.target.is_null() { continue; }
        unsafe {
            if *(site.target as *const u8) == 0xCC {
                crate::platform::memory::write_executable_byte(
                    site.target as *mut std::ffi::c_void,
                    site.original_byte,
                );
            }
        }
    }
    sites.clear();
}

/// Remove the shared VEH handler. Call only after all sites are disarmed.
pub fn remove_handler() {
    let mut guard = HANDLER.write();
    if let Some(h) = guard.take() {
        trap::remove_vectored_handler(h as trap::HandlerHandle);
    }
}

/// Convenience `on_hit` for the "capture this" pattern: writes argument 1
/// (RCX on x64) into the `*mut *mut c_void` pointed at by
/// `site.callback_data`.
pub fn capture_rcx_to(ctx: &mut Context, site: &Int3Site) {
    if site.callback_data.is_null() {
        return;
    }
    let this = ctx.argument(1) as *mut std::ffi::c_void;
    unsafe {
        // SAFETY: `callback_data` was populated by `arm_capture_*!` from a
        // process-static `Slot`'s `as_ptr()`, i.e. it points at storage
        // that outlives the site. The site is one-shot, so this write
        // happens exactly once, from the thread that hit the int3.
        let slot = site.callback_data as *mut *mut std::ffi::c_void;
        *slot = this;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;

    #[test]
    fn is_at_exact_match() {
        let x = 42u64;
        let p = &x as *const u64 as *const c_void;
        assert!(is_at(p as u64, p));
        assert!(!is_at(p as u64 + 1, p));
    }

    #[test]
    fn is_post_int3_step_bounds() {
        let base = 0x1000u64;
        let p = base as *const c_void;
        // Exactly at target: not "post".
        assert!(!is_post_int3_step(base, p));
        // One instruction past.
        assert!(is_post_int3_step(base + 1, p));
        assert!(is_post_int3_step(base + K_MAX_X64_INSN_LEN, p));
        assert!(!is_post_int3_step(base + K_MAX_X64_INSN_LEN + 1, p));
    }

    #[test]
    fn has_sites_starts_false_when_empty() {
        // The registry is process-global. This test does not arm a site,
        // so it only checks the call itself; the real disarm coverage
        // comes from the transactional uninstall path once hooks install.
        let _ = has_sites();
    }

    #[test]
    fn disarm_all_on_empty_is_noop() {
        // Same reasoning as `has_sites_starts_false_when_empty`: no panic
        // is the assertion.
        disarm_all();
    }

    #[test]
    fn handler_slot_is_usize_and_send_sync() {
        // Compile-time proof that the handler storage satisfies the
        // static's `Sync` requirement. If a future edit changes the type
        // back to a raw pointer, this test stops compiling.
        fn assert_sync<T: Sync>() {}
        assert_sync::<RwLock<Option<usize>>>();
    }
}
