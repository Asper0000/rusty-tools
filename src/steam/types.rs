//! Primitive typedefs and ID handles used throughout steamclient internals.
//!
//! Source: `Steam/Types.h`
//!
//! Layout and values are preserved exactly, including the wrapped values for
//! `k_uPackageIdWallet` and `k_uPackageIdMicroTxn` (C++ assigns `-2` and `-3`
//! to a `uint32_t`, which wraps to `0xFFFFFFFE` and `0xFFFFFFFD`).

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

pub type byte = u8;
pub type int32 = i32;
pub type int64 = i64;
pub type uint8 = u8;
pub type uint16 = u16;
pub type uint32 = u32;
pub type uint64 = u64;

pub type AppId_t = u32;
pub type PackageId_t = u32;
pub type PID_t = u32;
pub type AccountID_t = u32;
pub type HAuthTicket = u32;
pub type HCONNECTION = u32;

pub type HSteamPipe = i32;
pub type HSteamUser = i32;

pub type GID_t = u64;
pub type SteamAPICall_t = u64;
pub type JobID_t = GID_t;

pub const k_uAppIdInvalid: AppId_t = 0x0;
pub const k_uPackageIdFreeSub: PackageId_t = 0x0;
pub const k_uPackageIdInvalid: PackageId_t = 0xFFFF_FFFF;
/// C++: `PackageId_t k_uPackageIdWallet = -2;` -> wraps to 0xFFFFFFFE.
pub const k_uPackageIdWallet: PackageId_t = 0xFFFF_FFFE;
/// C++: `PackageId_t k_uPackageIdMicroTxn = -3;` -> wraps to 0xFFFFFFFD.
pub const k_uPackageIdMicroTxn: PackageId_t = 0xFFFF_FFFD;
pub const k_GIDNil: GID_t = 0xFFFF_FFFF_FFFF_FFFF;
pub const k_uAPICallInvalid: SteamAPICall_t = 0x0;

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn primitive_sizes_match_cpp() {
        assert_eq!(size_of::<byte>(), 1);
        assert_eq!(size_of::<int32>(), 4);
        assert_eq!(size_of::<int64>(), 8);
        assert_eq!(size_of::<uint8>(), 1);
        assert_eq!(size_of::<uint16>(), 2);
        assert_eq!(size_of::<uint32>(), 4);
        assert_eq!(size_of::<uint64>(), 8);
    }

    #[test]
    fn id_alias_sizes_match_cpp() {
        assert_eq!(size_of::<AppId_t>(), 4);
        assert_eq!(size_of::<PackageId_t>(), 4);
        assert_eq!(size_of::<PID_t>(), 4);
        assert_eq!(size_of::<AccountID_t>(), 4);
        assert_eq!(size_of::<HSteamPipe>(), 4);
        assert_eq!(size_of::<GID_t>(), 8);
        assert_eq!(size_of::<SteamAPICall_t>(), 8);
        assert_eq!(size_of::<JobID_t>(), 8);
    }

    #[test]
    fn wrapped_package_ids_match_cpp_semantics() {
        // C++ assigns -2 and -3 to uint32_t. The wrapped values are what
        // the C++ code actually observes at runtime.
        assert_eq!(k_uPackageIdWallet, 0xFFFF_FFFEu32);
        assert_eq!(k_uPackageIdMicroTxn, 0xFFFF_FFFDu32);
    }

    #[test]
    fn nil_constants() {
        assert_eq!(k_GIDNil, u64::MAX);
        assert_eq!(k_uAPICallInvalid, 0);
        assert_eq!(k_uAppIdInvalid, 0);
    }
}
