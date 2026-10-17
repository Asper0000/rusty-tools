//! In-memory structures mirrored from steamclient internals.
//! Source: `Steam/Structs.h`
//!
//! Layout-critical. All `#[repr(C)]` or `#[repr(C, packed)]` to match the
//! C++ `#pragma pack` regions. Bitfield containers are modelled as plain
//! integers with explicit mask/shift accessors because Rust has no
//! bitfields.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]

use core::ffi::c_void;
use super::types::*;
use super::enums::*;

// ─── CUtlMemory<T> ──────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CUtlMemory<T> {
    pub m_pMemory: *mut T,
    pub m_nAllocationCount: u32,
    pub m_nGrowSize: u32,
}

// ─── CUtlVector<T> ──────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct CUtlVector<T> {
    pub m_Memory: CUtlMemory<T>,
    pub m_Size: u32,
}

impl<T: Copy + PartialEq> CUtlVector<T> {
    /// Swap-with-last removal, then shrink. Order is not preserved.
    /// Matches the C++ method exactly, including the no-op if `elem >= m_Size`.
    pub unsafe fn fast_remove(&mut self, elem: u32) {
        if elem < self.m_Size {
            if elem != self.m_Size - 1 {
                let src = self.m_Memory.m_pMemory.add((self.m_Size - 1) as usize);
                let dst = self.m_Memory.m_pMemory.add(elem as usize);
                *dst = *src;
            }
            self.m_Size -= 1;
        }
    }

    pub unsafe fn find_and_fast_remove(&mut self, src: &T) -> bool {
        for i in 0..self.m_Size {
            if *self.m_Memory.m_pMemory.add(i as usize) == *src {
                self.fast_remove(i);
                return true;
            }
        }
        false
    }
}

// ─── CUtlBuffer ─────────────────────────────────────────────────────
//
// The C++ version carries two member-function pointers used to grow the
// buffer. We model them as free-function pointers of the same size (8
// bytes on x64). The C++ code never calls them via our port; only the
// layout matters. If call sites eventually need to invoke them we can
// switch to a `this`-aware calling convention.

#[repr(C)]
#[derive(Copy, Clone)]
pub struct CUtlBuffer {
    pub m_Memory: CUtlMemory<u8>,
    pub m_Get: i32,
    pub m_Put: i32,
    pub m_nOffset: i32,
    pub m_flags: i32,
    pub m_GetOverflowFunc: Option<unsafe extern "C" fn(i32) -> bool>,
    pub m_PutOverflowFunc: Option<unsafe extern "C" fn(i32) -> bool>,
}

impl CUtlBuffer {
    pub fn base(&self) -> *mut u8 { self.m_Memory.m_pMemory }
    pub fn tell_put(&self) -> i32 { self.m_Put }
    pub fn tell_get(&self) -> i32 { self.m_Get }
    pub fn size(&self) -> u32 { self.m_Memory.m_nAllocationCount }
    pub fn capacity(&self) -> u32 { self.m_Memory.m_nAllocationCount }
}

// ─── PackageInfo ────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PackageInfo {
    pub PackageId: AppId_t,
    pub ChangeNumber: i32,
    pub PICS_token: u64,
    pub BillingType: BillingType,
    pub LicenseType: ELicenseType,
    pub Status: EPackageStatus,
    pub SHA_1_Hash: [u8; 20],
    pub pPackageInfoNodeBegin: *mut c_void,
    pub pExtendNodeBegin: *mut c_void,
    pub AppIdVec: CUtlVector<AppId_t>,
    pub DepotIdVec: CUtlVector<AppId_t>,
}

// ─── AppOwnership ───────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AppOwnership {
    pub PackageId: PackageId_t,
    pub ReleaseState: EAppReleaseState,
    pub SteamId32: AccountID_t,
    pub MasterSubscriptionAppID: AppId_t,
    pub TrialSeconds: u32,
    pub ExistInPackageNums: u32,
    pub PurchaseCountryCode: [i8; 4],
    pub TimeStamp: u32,
    pub TimeExpire: u32,
    pub bOwnsLicense: bool,
    pub bLicenseExpired: bool,
    pub bIsPermanent: bool,
    pub bLowViolence: bool,
    pub bFreeLicense: bool,
    pub bRegionRestricted: bool,
    pub bFromFreeWeekend: bool,
    pub bLicenseLocked: bool,
    pub bLicensePending: bool,
    pub bRetailLicense: bool,
    pub bAutoGrant: bool,
    pub bLicensePermanent: bool,
    pub bGuestPass: bool,
    pub bBorrowed: bool,
    pub bAnySiteLicense: bool,
    pub bAllSiteLicenses: bool,
    pub bAllActivationRequired: bool,
    pub bFamilyShared: bool,
}

// ─── DepotEntry ─────────────────────────────────────────────────────
//
// static_assert(sizeof(DepotEntry) == 0x20) in C++.

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct DepotEntry {
    pub DepotId: u32,
    pub AppId: u32,
    pub ManifestGid: u64,
    pub ManifestSize: u64,
    pub DlcAppId: u32,
    pub LcsRequired: u8,
    pub bNotNewTarget: u8,
    pub SharedInstall: u8,
    pub Padding: u8,
}

const _: () = assert!(core::mem::size_of::<DepotEntry>() == 0x20);

// ─── KeyValues ──────────────────────────────────────────────────────
//
// static_assert(sizeof(KeyValues) == 0x20) in C++.

#[repr(C)]
#[derive(Copy, Clone)]
pub union KeyValuesValue {
    pub m_pSub: *mut KeyValues,
    pub m_sValue: *mut i8,
    pub m_wsValue: *mut u16,
    pub m_iValue: i32,
    pub m_flValue: f32,
    pub m_pValue: *mut c_void,
    pub m_ullValue: u64,
    pub m_llValue: i64,
    pub m_Color: [u8; 8],
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct KeyValues {
    pub value: KeyValuesValue,
    pub m_pChain: *mut KeyValues,
    /// Packed bitfield container:
    ///   bits  0..24 (25)  m_iKeyName
    ///   bits 25..28 ( 4)  m_iDataType
    ///   bit      29 ( 1)  m_bHasEscapeSequences
    ///   bit      30 ( 1)  m_bEvaluateConditionals
    ///   bit      31 ( 1)  m_bAllocatedValue
    pub m_iPackedKeyAndType: u32,
    pub m_unFlags: u32,
    pub m_pPeer: *mut KeyValues,
}

impl KeyValues {
    pub const fn key_name(&self) -> u32 { self.m_iPackedKeyAndType & 0x01FF_FFFF }
    pub const fn data_type(&self) -> u32 { (self.m_iPackedKeyAndType >> 25) & 0x0F }
    pub const fn has_escape_sequences(&self) -> bool { (self.m_iPackedKeyAndType >> 29) & 1 != 0 }
    pub const fn evaluate_conditionals(&self) -> bool { (self.m_iPackedKeyAndType >> 30) & 1 != 0 }
    pub const fn allocated_value(&self) -> bool { (self.m_iPackedKeyAndType >> 31) & 1 != 0 }
}

const _: () = assert!(core::mem::size_of::<KeyValues>() == 0x20);

// ─── IKeyValuesSystem ───────────────────────────────────────────────
//
// Vtable-based C++ interface. We model the vtable explicitly and expose
// safe wrappers for the two methods the codebase uses.

#[repr(C)]
pub struct IKeyValuesSystemVtable {
    pub register_sizeof_keyvalues: unsafe extern "C" fn(this: *mut IKeyValuesSystem, size: i32),
    pub get_symbol_for_string: unsafe extern "C" fn(this: *mut IKeyValuesSystem, name: *const i8, create: bool) -> i32,
    pub get_string_for_symbol: unsafe extern "C" fn(this: *mut IKeyValuesSystem, symbol: i32) -> *const i8,
}

#[repr(C)]
pub struct IKeyValuesSystem {
    pub vtable: *const IKeyValuesSystemVtable,
}

impl IKeyValuesSystem {
    pub unsafe fn get_symbol(&self, name: *const i8) -> i32 {
        ((*self.vtable).get_symbol_for_string)(self as *const _ as *mut _, name, false)
    }
    pub unsafe fn get_or_create_symbol(&self, name: *const i8) -> i32 {
        ((*self.vtable).get_symbol_for_string)(self as *const _ as *mut _, name, true)
    }
    pub unsafe fn get_key_name(&self, symbol: i32) -> *const i8 {
        ((*self.vtable).get_string_for_symbol)(self as *const _ as *mut _, symbol)
    }
}

pub type KeyValuesSystemSteam_t = unsafe extern "C" fn() -> *mut IKeyValuesSystem;

// ─── CNetPacket / MsgHdr / ExtendedMsgHdr ───────────────────────────

/// Opaque. Layout is detected at runtime via `NetPkt`.
#[repr(C)]
pub struct CNetPacket {
    _opaque: [u8; 0],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct MsgHdr {
    pub eMsg: u32,
    pub headerLength: u32,
}

pub const kMsgHdrProtoFlag: u32 = 0x8000_0000;

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct ExtendedMsgHdr {
    pub eMsg: u32,             // EMsg | 0x8000_0000 (flag stays clear for non-proto)
    pub m_nCubHdr: u8,
    pub m_nHdrVersion: u16,
    pub m_JobIDTarget: JobID_t,
    pub m_JobIDSource: JobID_t,
    pub m_nHdrCanary: u8,
    pub m_ulSteamID: u64,
    pub m_nSessionID: i32,
}

// These offsets are hardcoded as raw byte indices in legacy_key.rs; the
// asserts fail the build if the struct ever drifts from the layout the
// caller assumes.
const _: () = assert!(core::mem::offset_of!(ExtendedMsgHdr, m_JobIDTarget) == 7);
const _: () = assert!(core::mem::offset_of!(ExtendedMsgHdr, m_JobIDSource) == 15);
const _: () = assert!(core::mem::offset_of!(ExtendedMsgHdr, m_ulSteamID) == 24);

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct MsgClientGetLegacyGameKey {
    pub m_unAppId: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct MsgClientGetLegacyGameKeyResponse {
    pub m_unAppId: u32,
    pub m_eResult: i32, // EResult
    pub m_cchKey: u32,
}

// ─── CPipeClient ────────────────────────────────────────────────────

#[repr(C)]
pub struct CPipeClient {
    pub m_pServer: *mut c_void,
    pub m_pClient: *mut c_void,
    pub m_hSteamPipe: HSteamPipe,
    pub _pad0: [u8; 12],
    pub m_clientPID: PID_t,
    pub _pad1: [u8; 4],
    pub m_szProcessName: *mut i8,
    pub _pad2: [u8; 80],
    pub m_nActiveRefs: i32,
    pub _pad3: [u8; 188],
    pub m_pLocalIPCServer: *mut c_void,
    pub m_bInProcess: bool,
    pub _pad4: [u8; 31],
}

// ─── CGameID ────────────────────────────────────────────────────────
//
// Union of a u64 and a struct with bitfields:
//   m_nAppID : 24   bits 0..23
//   m_nType  :  8   bits 24..31
//   m_nModID : 32   bits 32..63
// MSVC x64 packs bitfields from the least-significant bit up.

#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq)]
pub struct CGameID {
    pub m_ulGameID: u64,
}

impl CGameID {
    pub const APP_ID_MASK: u64 = 0x00FF_FFFF;
    pub const TYPE_SHIFT: u32 = 24;

    pub fn app_id(&self, check_type: bool) -> AppId_t {
        if check_type && !self.is_steam_app() { return 0; }
        (self.m_ulGameID & Self::APP_ID_MASK) as AppId_t
    }
    pub fn set_app_id(&mut self, app_id: AppId_t) {
        self.m_ulGameID = (self.m_ulGameID & !Self::APP_ID_MASK)
            | ((app_id as u64) & Self::APP_ID_MASK);
    }
    pub fn is_steam_app(&self) -> bool {
        ((self.m_ulGameID >> Self::TYPE_SHIFT) & 0xFF) == 0
    }
}

const _: () = assert!(core::mem::size_of::<CGameID>() == 8);

// ─── CSteamID ───────────────────────────────────────────────────────
//
// Union of a u64 and a struct with bitfields:
//   m_unAccountID        : 32   bits  0..31
//   m_unAccountInstance  : 20   bits 32..51
//   m_EAccountType       :  4   bits 52..55
//   m_EUniverse          :  8   bits 56..63

#[repr(transparent)]
#[derive(Copy, Clone, Default, PartialEq, Eq, Hash)]
pub struct CSteamID {
    pub m_unAll64Bits: u64,
}

pub const k_unSteamAccountIDMask: u32 = 0xFFFF_FFFF;
pub const k_unSteamAccountInstanceMask: u32 = 0x000F_FFFF;
pub const k_unSteamUserDefaultInstance: u32 = 1;

impl CSteamID {
    pub fn new() -> Self { Self::default() }
    pub fn from_u64(id: u64) -> Self { Self { m_unAll64Bits: id } }
    pub fn convert_to_u64(&self) -> u64 { self.m_unAll64Bits }

    pub fn set_from_u64(&mut self, id: u64) { self.m_unAll64Bits = id; }

    pub fn set(&mut self, account_id: u32, universe: EUniverse, account_type: EAccountType) {
        let instance: u64 = if matches!(account_type, EAccountType::Clan | EAccountType::GameServer) {
            0
        } else {
            k_unSteamUserDefaultInstance as u64
        };
        let mut v = 0u64;
        v |= account_id as u64;
        v |= (instance & 0x000F_FFFF) << 32;
        v |= ((account_type as u64) & 0x0F) << 52;
        v |= ((universe as u64) & 0xFF) << 56;
        self.m_unAll64Bits = v;
    }

    pub fn account_id(&self) -> AccountID_t {
        (self.m_unAll64Bits & 0xFFFF_FFFF) as AccountID_t
    }
    pub fn set_account_id(&mut self, id: AccountID_t) {
        self.m_unAll64Bits = (self.m_unAll64Bits & !0xFFFF_FFFF) | (id as u64);
    }
    pub fn account_instance(&self) -> u32 {
        ((self.m_unAll64Bits >> 32) & 0x000F_FFFF) as u32
    }
    pub fn account_type_raw(&self) -> u8 {
        ((self.m_unAll64Bits >> 52) & 0x0F) as u8
    }
    pub fn universe_raw(&self) -> u8 {
        ((self.m_unAll64Bits >> 56) & 0xFF) as u8
    }
}

impl core::fmt::Debug for CSteamID {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "CSteamID({})", self.m_unAll64Bits)
    }
}

impl core::fmt::Display for CSteamID {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.m_unAll64Bits)
    }
}

const _: () = assert!(core::mem::size_of::<CSteamID>() == 8);

// ─── CAppData ───────────────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone)]
pub struct CAppData {
    pub vfptr: *mut *mut c_void,
    pub nAppID: AppId_t,
    pub ChangeNumber: u32,
    pub LastChangeTimeStamp: u32,
    pub bSkipFlag: bool,
    pub bDeniedToken: bool,
    pub bMissingToken: bool,
    pub accessToken: u64,
    pub sha1Hash: [u8; 20],
}

impl CAppData {
    pub fn has_empty_app_info_sha(&self) -> bool {
        self.sha1Hash.iter().all(|&b| b == 0)
    }
    pub fn is_unresolved_app_info(&self) -> bool {
        self.has_empty_app_info_sha() && !self.bSkipFlag
    }
}

// ─── CSteamApp ──────────────────────────────────────────────────────
//
// Wrapped in `#pragma pack(push,1)` / `#pragma pack(pop)` in C++.

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct CSteamApp {
    pub vfptr: *mut *mut c_void,
    pub GameID: CGameID,
    pub nAppID: AppId_t,
    pub _unknown1: u16,
    pub _unknown2: u16,
    pub ReleaseState: EAppReleaseState,
    pub OwnershipFlags: EAppOwnershipFlags,
    pub AppStateFlags: EAppState,
    pub SteamID: CSteamID,
    pub PurchasedTime: u32,
    pub ChangeNumber: u32,
    pub LicenseExpirationTime: u32,
    pub MasterSubAppID: AppId_t,
    pub eProtoAppType: u32,
    pub ParentAppID: AppId_t,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn depot_entry_layout() {
        assert_eq!(size_of::<DepotEntry>(), 0x20);
    }

    #[test]
    fn keyvalues_layout() {
        assert_eq!(size_of::<KeyValues>(), 0x20);
    }

    #[test]
    fn cgameid_roundtrip() {
        let mut g = CGameID::default();
        g.set_app_id(730);
        assert_eq!(g.app_id(false), 730);
        assert!(g.is_steam_app());
        g.m_ulGameID |= 1 << CGameID::TYPE_SHIFT; // set type = 1 (GameMod)
        assert!(!g.is_steam_app());
        assert_eq!(g.app_id(true), 0);
        assert_eq!(g.app_id(false), 730);
    }

    #[test]
    fn csteamid_set_and_extract() {
        let mut s = CSteamID::new();
        s.set(0x1234_5678, EUniverse::Public, EAccountType::Individual);
        assert_eq!(s.account_id(), 0x1234_5678);
        assert_eq!(s.account_instance(), k_unSteamUserDefaultInstance);
        assert_eq!(s.account_type_raw(), EAccountType::Individual as u8);
        assert_eq!(s.universe_raw(), EUniverse::Public as u8);

        let mut c = CSteamID::new();
        c.set(999, EUniverse::Public, EAccountType::Clan);
        assert_eq!(c.account_instance(), 0); // Clan -> no instance
    }

        #[test]
    fn csteamid_round_trip_through_u64() {
        let mut s = CSteamID::new();
        s.set(0xDEAD_BEEF, EUniverse::Public, EAccountType::Individual);
        let raw = s.convert_to_u64();
        let s2 = CSteamID::from_u64(raw);
        assert_eq!(s2.account_id(), 0xDEAD_BEEF);
    }

    #[test]
    fn keyvalues_bitfield_accessors() {
        let mut kv: KeyValues = unsafe { core::mem::zeroed() };
        kv.m_iPackedKeyAndType = 0;
        // Bits 0..24: key name
        kv.m_iPackedKeyAndType |= 0x1_2345;
        assert_eq!(kv.key_name(), 0x1_2345);
        // Bits 25..28: data type
        kv.m_iPackedKeyAndType |= 7 << 25;
        assert_eq!(kv.data_type(), 7);
        // Bit 29: escape sequences
        kv.m_iPackedKeyAndType |= 1 << 29;
        assert!(kv.has_escape_sequences());
        // Bit 30: evaluate conditionals
        kv.m_iPackedKeyAndType |= 1 << 30;
        assert!(kv.evaluate_conditionals());
        // Bit 31: allocated value
        kv.m_iPackedKeyAndType |= 1u32 << 31;
        assert!(kv.allocated_value());
    }

    #[test]
    fn extended_msg_hdr_is_packed() {
        // 4 + 1 + 2 + 8 + 8 + 1 + 8 + 4 = 36 bytes when packed.
        assert_eq!(size_of::<ExtendedMsgHdr>(), 36);
    }

    #[test]
    fn cpipclient_size_from_padding() {
        // Sum: 8 + 8 + 4 + 12 + 4 + 4 + 8 + 80 + 4 + 188 + 8 + 1 + 31 = 360.
        assert_eq!(size_of::<CPipeClient>(), 360);
    }

    #[test]
    fn capp_data_empty_sha_check() {
        let d: CAppData = unsafe { core::mem::zeroed() };
        assert!(d.has_empty_app_info_sha());
        assert!(d.is_unresolved_app_info());

        let mut d2 = d;
        d2.sha1Hash[0] = 1;
        assert!(!d2.has_empty_app_info_sha());
        assert!(!d2.is_unresolved_app_info());
    }

    #[test]
    fn cutlvector_fast_remove_logic() {
        // Heap-allocate a small array, wire it into a CUtlVector, and
        // exercise fast_remove against the raw pointer.
        let mut storage: [u32; 4] = [10, 20, 30, 40];
        let mut v = CUtlVector::<u32> {
            m_Memory: CUtlMemory {
                m_pMemory: storage.as_mut_ptr(),
                m_nAllocationCount: 4,
                m_nGrowSize: 0,
            },
            m_Size: 4,
        };
        unsafe {
            assert!(v.find_and_fast_remove(&30));
            assert_eq!(v.m_Size, 3);
            // 30 was at index 2; fast_remove swapped in the last element (40).
            assert_eq!(*v.m_Memory.m_pMemory.add(0), 10);
            assert_eq!(*v.m_Memory.m_pMemory.add(1), 20);
            assert_eq!(*v.m_Memory.m_pMemory.add(2), 40);
        }
    }
}
