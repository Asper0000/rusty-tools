//! NtDLL ABI surface used for process inspection.
//! Source: `OSTPlatform/Windows/NtAbi.h`
//!
//! These structures are walked to read another process's PEB and
//! command line / environment. All offsets are checked against the
//! `static_assert`s in the C++ header.
//!
//! ## Stub phase
//!
//! Only the types are here. The functions that consume them
//! (`NtQueryInformationProcess`, `NtQueryVirtualMemory`) will be
//! resolved by `Process.cpp` in sub-batch 2f, which is where the FFI
//! bindings for `ntdll.dll` are added.

use std::ffi::c_void;

// ─── Windows ABI primitives ─────────────────────────────────────────

pub type BYTE      = u8;
pub type ULONG     = u32;
pub type ULONG_PTR = usize;
pub type LONG      = i32;
pub type NTSTATUS  = i32;
pub type SIZE_T    = usize;

pub type PVOID   = *mut c_void;
pub type PULONG  = *mut ULONG;
pub type PSIZE_T = *mut SIZE_T;

/// `HANDLE` - kept as a raw pointer for the same reason as in
/// [`super::handles`].
pub type HANDLE = *mut c_void;

// ─── Enums ──────────────────────────────────────────────────────────

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProcessInfoClass {
    BasicInformation = 0,
    Wow64Information = 26,
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MemoryInfoClass {
    BasicInformation = 0,
}

// ─── Return type ────────────────────────────────────────────────────
//
// `NtSuccess` is a signed comparison: `NTSTATUS` is `LONG`, and values
// with the high bit set (`STATUS_UNSUCCESSFUL = 0xC0000001`, etc.) are
// errors. `status >= 0` is the C++ rule; keep it identical.

#[inline]
pub const fn nt_success(status: NTSTATUS) -> bool {
    status >= 0
}

// ─── Structs ────────────────────────────────────────────────────────
//
// All `#[repr(C)]`. Offset asserts below match the C++ `static_assert`s.

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ProcessBasicInformation {
    pub exit_status: NTSTATUS,
    // 4 bytes padding on x64 before the 8-byte pointer
    pub peb_base_address: PVOID,
    pub affinity_mask: ULONG_PTR,
    pub base_priority: LONG,
    // 4 bytes padding on x64 before the 8-byte ULONG_PTR
    pub unique_process_id: ULONG_PTR,
    pub inherited_from_unique_process_id: ULONG_PTR,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct UnicodeString {
    pub length: u16,
    pub maximum_length: u16,
    pub padding: u32,
    pub buffer: PVOID,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct UnicodeString32 {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Peb {
    pub reserved0: [BYTE; 0x20],
    pub process_parameters: PVOID,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct RtlUserProcessParameters {
    pub reserved0: [BYTE; 0x70],
    pub command_line: UnicodeString,  // offset 0x70
    pub environment: PVOID,           // offset 0x80
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Peb32 {
    pub reserved0: [BYTE; 0x10],
    pub process_parameters: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct RtlUserProcessParameters32 {
    pub reserved0: [BYTE; 0x40],
    pub command_line: UnicodeString32,  // offset 0x40
    pub environment: u32,               // offset 0x48
}

// ─── Offset asserts (mirror the C++ static_asserts) ─────────────────

const _: () = assert!(core::mem::offset_of!(Peb, process_parameters) == 0x20);
const _: () = assert!(core::mem::offset_of!(RtlUserProcessParameters, command_line) == 0x70);
const _: () = assert!(core::mem::offset_of!(RtlUserProcessParameters, environment) == 0x80);
const _: () = assert!(core::mem::offset_of!(Peb32, process_parameters) == 0x10);
const _: () = assert!(core::mem::offset_of!(RtlUserProcessParameters32, command_line) == 0x40);
const _: () = assert!(core::mem::offset_of!(RtlUserProcessParameters32, environment) == 0x48);

// ─── Function pointer types ─────────────────────────────────────────

/// `NTAPI` is `__stdcall`. On x64 there is only one calling convention,
/// so `extern "system"` is correct on both targets.
pub type NtQueryInformationProcessFn = unsafe extern "system" fn(
    HANDLE,
    ProcessInfoClass,
    PVOID,
    ULONG,
    PULONG,
) -> NTSTATUS;

pub type NtQueryVirtualMemoryFn = unsafe extern "system" fn(
    HANDLE,
    PVOID,
    MemoryInfoClass,
    PVOID,
    SIZE_T,
    PSIZE_T,
) -> NTSTATUS;

// ─── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn process_info_class_values() {
        assert_eq!(ProcessInfoClass::BasicInformation as u32, 0);
        assert_eq!(ProcessInfoClass::Wow64Information as u32, 26);
    }

    #[test]
    fn memory_info_class_values() {
        assert_eq!(MemoryInfoClass::BasicInformation as u32, 0);
    }

    #[test]
    fn nt_success_matches_cpp_rule() {
        assert!(nt_success(0));
        assert!(nt_success(1));
        assert!(nt_success(0x7FFF_FFFF)); // STATUS_SUCCESS upper bound
        assert!(!nt_success(-1));
        // STATUS_UNSUCCESSFUL = 0xC0000001 => as i32 it is negative
        assert!(!nt_success(0xC000_0001u32 as i32));
    }

    #[test]
    fn ntstatus_is_i32() {
        assert_eq!(size_of::<NTSTATUS>(), 4);
    }

    #[test]
    fn ulong_and_long_are_4_bytes() {
        assert_eq!(size_of::<ULONG>(), 4);
        assert_eq!(size_of::<LONG>(), 4);
    }

    #[test]
    fn ulong_ptr_and_size_t_match_pointer_width() {
        assert_eq!(size_of::<ULONG_PTR>(), size_of::<usize>());
        assert_eq!(size_of::<SIZE_T>(), size_of::<usize>());
    }

    #[test]
    fn unicode_string_layout() {
        assert_eq!(size_of::<UnicodeString>(), 16);
        assert_eq!(offset_of!(UnicodeString, length), 0);
        assert_eq!(offset_of!(UnicodeString, maximum_length), 2);
        assert_eq!(offset_of!(UnicodeString, padding), 4);
        assert_eq!(offset_of!(UnicodeString, buffer), 8);
    }

    #[test]
    fn unicode_string32_layout() {
        assert_eq!(size_of::<UnicodeString32>(), 8);
        assert_eq!(offset_of!(UnicodeString32, buffer), 4);
    }

    #[test]
    fn peb_layout() {
        assert_eq!(offset_of!(Peb, process_parameters), 0x20);
        assert_eq!(size_of::<Peb>(), 0x28);
    }

    #[test]
    fn peb32_layout() {
        assert_eq!(offset_of!(Peb32, process_parameters), 0x10);
        assert_eq!(size_of::<Peb32>(), 0x14);
    }

    #[test]
    fn rtl_user_process_parameters_layout() {
        assert_eq!(offset_of!(RtlUserProcessParameters, command_line), 0x70);
        assert_eq!(offset_of!(RtlUserProcessParameters, environment), 0x80);
        assert_eq!(size_of::<RtlUserProcessParameters>(), 0x88);
    }

    #[test]
    fn rtl_user_process_parameters32_layout() {
        assert_eq!(offset_of!(RtlUserProcessParameters32, command_line), 0x40);
        assert_eq!(offset_of!(RtlUserProcessParameters32, environment), 0x48);
        assert_eq!(size_of::<RtlUserProcessParameters32>(), 0x4C);
    }

    #[test]
    fn process_basic_information_layout() {
        // exit_status(4) pad(4) ptr(8) ptr(8) long(4) pad(4) ptr(8) ptr(8) = 0x30
        assert_eq!(offset_of!(ProcessBasicInformation, exit_status), 0);
        assert_eq!(offset_of!(ProcessBasicInformation, peb_base_address), 8);
        assert_eq!(offset_of!(ProcessBasicInformation, affinity_mask), 16);
        assert_eq!(offset_of!(ProcessBasicInformation, base_priority), 24);
        assert_eq!(offset_of!(ProcessBasicInformation, unique_process_id), 32);
        assert_eq!(offset_of!(ProcessBasicInformation, inherited_from_unique_process_id), 40);
        assert_eq!(size_of::<ProcessBasicInformation>(), 48);
    }

    #[test]
    fn function_pointers_are_pointer_sized() {
        assert_eq!(size_of::<NtQueryInformationProcessFn>(), size_of::<usize>());
        assert_eq!(size_of::<NtQueryVirtualMemoryFn>(), size_of::<usize>());
    }
}
