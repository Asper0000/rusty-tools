//! Runtime detection of `CNetPacket`'s `m_pubData` / `m_cubData` offsets.
//! Source: `Steam/NetPacket.h`
//!
//! The two candidates differ by 8 (the beta client inserted two uint32
//! version stamps after `m_hConnection`). Everything else is unchanged.
//!
//! ## API shape
//!
//! The C++ header returns `uint8*&` / `uint32&` - references to the fields
//! inside the live packet. Every call site in the codebase uses these as a
//! plain load/store pair:
//!
//! ```text
//!   uint8* orig = NetPkt::Data(p);
//!   NetPkt::Data(p) = replacement;
//!   ... call the original ...
//!   NetPkt::Data(p) = orig;
//! ```
//!
//! Rust has no safe way to hand out a `&mut *mut u8` at an arbitrary
//! byte offset inside a `#[repr(...)]`-opaque object without inviting
//! misaligned-reference UB. Instead we expose the four operations the
//! call sites actually need - [`read_data`], [`write_data`], [`read_size`],
//! [`write_size`] - each of which uses `read_unaligned` / `write_unaligned`
//! and therefore accepts packets at any alignment.
//!
//! When the layout is unresolved or disabled, all four route through a
//! single scratch slot. Reads see a null pointer / zero; writes are
//! discarded. That mirrors the "trash" fallback in the C++ version, so a
//! call added above the gate corrupts a dead global instead of a live
//! Steam object.

use core::sync::atomic::{AtomicU32, Ordering};

#[derive(Copy, Clone, Debug)]
pub struct Layout {
    pub name: &'static str,
    pub data_off: u32,
}

pub const K_LAYOUTS: &[Layout] = &[
    Layout { name: "stable", data_off: 0x08 },
    Layout { name: "beta",   data_off: 0x10 },
];

pub const K_UNRESOLVED: u32 = 0;
pub const K_DISABLED: u32 = 0xFFFF_FFFF;

static G_DATA_OFF: AtomicU32 = AtomicU32::new(K_UNRESOLVED);

#[inline]
pub fn state() -> u32 { G_DATA_OFF.load(Ordering::Relaxed) }

#[inline]
pub fn is_resolved() -> bool {
    let v = state();
    v != K_UNRESOLVED && v != K_DISABLED
}

#[inline]
pub fn is_disabled() -> bool { state() == K_DISABLED }

#[inline]
pub fn latch(data_off: u32) { G_DATA_OFF.store(data_off, Ordering::Relaxed); }

#[inline]
pub fn disable() { G_DATA_OFF.store(K_DISABLED, Ordering::Relaxed); }

/// Scratch slot returned when the layout is unresolved. A plain static,
/// not thread-local: Steam threads predate our injection, and races on a
/// discard sink are harmless by construction.
pub mod detail {
    pub static mut TRASH_DATA: *mut u8 = core::ptr::null_mut();
    pub static mut TRASH_SIZE: u32 = 0;
}

/// Read the `m_pubData` pointer out of the packet.
///
/// # Safety
///
/// `p` must point to at least `off + 8` bytes of readable memory, or the
/// layout must be unresolved/disabled (in which case a scratch slot is
/// returned and `p` is not touched).
#[inline]
pub unsafe fn read_data(p: *mut u8) -> *mut u8 {
    let off = state();
    if p.is_null() || off == K_UNRESOLVED || off == K_DISABLED {
        return detail::TRASH_DATA;
    }
    core::ptr::read_unaligned(p.add(off as usize) as *const *mut u8)
}

/// Write the `m_pubData` pointer inside the packet.
///
/// # Safety
///
/// Same requirements as [`read_data`].
#[inline]
pub unsafe fn write_data(p: *mut u8, value: *mut u8) {
    let off = state();
    if p.is_null() || off == K_UNRESOLVED || off == K_DISABLED {
        detail::TRASH_DATA = value;
        return;
    }
    core::ptr::write_unaligned(p.add(off as usize) as *mut *mut u8, value);
}

/// Read the `m_cubData` u32 out of the packet.
///
/// # Safety
///
/// Same requirements as [`read_data`], with the range extended by 12 bytes
/// (the u32 plus the cRef that follows).
#[inline]
pub unsafe fn read_size(p: *mut u8) -> u32 {
    let off = state();
    if p.is_null() || off == K_UNRESOLVED || off == K_DISABLED {
        return detail::TRASH_SIZE;
    }
    core::ptr::read_unaligned(p.add(off as usize + 8) as *const u32)
}

/// Write the `m_cubData` u32 inside the packet.
///
/// # Safety
///
/// Same requirements as [`read_size`].
#[inline]
pub unsafe fn write_size(p: *mut u8, value: u32) {
    let off = state();
    if p.is_null() || off == K_UNRESOLVED || off == K_DISABLED {
        detail::TRASH_SIZE = value;
        return;
    }
    core::ptr::write_unaligned(p.add(off as usize + 8) as *mut u32, value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset() {
        G_DATA_OFF.store(K_UNRESOLVED, Ordering::Relaxed);
        unsafe {
            detail::TRASH_DATA = core::ptr::null_mut();
            detail::TRASH_SIZE = 0;
        }
    }

    #[test]
    fn latch_then_resolved() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        assert!(!is_resolved());
        assert!(!is_disabled());
        latch(0x08);
        assert!(is_resolved());
        assert!(!is_disabled());
        assert_eq!(state(), 0x08);
    }

    #[test]
    fn disable_is_terminal() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        latch(0x10);
        disable();
        assert!(is_disabled());
        assert!(!is_resolved());
        assert_eq!(state(), K_DISABLED);
    }

    #[test]
    fn read_write_when_unresolved_uses_trash() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        // A byte-aligned buffer - deliberately not 8-aligned - so any
        // attempt to form an aligned reference would be caught.
        let mut packet = [0u8; 0x40];
        unsafe {
            write_data(packet.as_mut_ptr(), 0x1234 as *mut u8);
            write_size(packet.as_mut_ptr(), 0x5678);
            assert_eq!(read_data(packet.as_mut_ptr()) as usize, 0x1234);
            assert_eq!(read_size(packet.as_mut_ptr()), 0x5678);
            // packet bytes must be untouched
            assert!(packet.iter().all(|&b| b == 0));
        }
    }

    #[test]
    fn read_write_at_stable_offset() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        latch(0x08);
        // Byte-aligned buffer. `write_unaligned` / `read_unaligned` must
        // handle this without UB.
        let mut buf = [0u8; 0x20];
        let mut target: u8 = 0xAB;
        unsafe {
            let p = buf.as_mut_ptr();
            write_data(p, &mut target as *mut u8);
            write_size(p, 42);
            assert_eq!(read_data(p) as *mut u8, &mut target as *mut u8);
            assert_eq!(read_size(p), 42);
            // Confirm the bytes actually landed where expected.
            let stored = core::ptr::read_unaligned(p.add(0x08) as *const *mut u8);
            assert_eq!(stored as *mut u8, &mut target as *mut u8);
            let stored_size = core::ptr::read_unaligned(p.add(0x10) as *const u32);
            assert_eq!(stored_size, 42);
        }
    }

    #[test]
    fn read_write_at_beta_offset() {
        let _g = TEST_LOCK.lock().unwrap();
        reset();
        latch(0x10);
        let mut buf = [0u8; 0x30];
        let mut target: u8 = 0xCD;
        unsafe {
            let p = buf.as_mut_ptr();
            write_data(p, &mut target as *mut u8);
            write_size(p, 99);
            assert_eq!(read_data(p) as *mut u8, &mut target as *mut u8);
            assert_eq!(read_size(p), 99);
            let stored = core::ptr::read_unaligned(p.add(0x10) as *const *mut u8);
            assert_eq!(stored as *mut u8, &mut target as *mut u8);
            let stored_size = core::ptr::read_unaligned(p.add(0x18) as *const u32);
            assert_eq!(stored_size, 99);
        }
    }
}
