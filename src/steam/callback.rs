//! Callback structs the codebase intercepts or injects.
//! Source: `Steam/Callback.h`

#![allow(non_camel_case_types)]

use super::types::*;
use super::enums::EResult;

pub const K_ISTEAM_USER_CALLBACKS: i32 = 100;

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct EncryptedAppTicketResponse_t {
    pub m_eResult: EResult,
}

impl EncryptedAppTicketResponse_t {
    pub const K_ICALLBACK: i32 = K_ISTEAM_USER_CALLBACKS + 54;
}

/// Broadcast when app licenses change. Sent by CClientAppManager after
/// ProcessPendingLicenseUpdates.
///
/// Layout under the default C++ alignment (this struct is NOT packed):
///   bool      m_bReloadAll          0x00
///   bool      m_bIsFirstLoad        0x01
///   (2 bytes pad)
///   uint32    m_unRemainingPackets  0x04
///   uint32    m_unCount             0x08
///   AppId_t   m_rgAppsUpdated[64]   0x0C  (256 bytes)
///   (4 bytes pad to align u64)
///   uint64    m_unAppsAdded         0x110
/// Total: 0x118 bytes.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct AppLicensesChanged_t {
    pub m_bReloadAll: bool,
    pub m_bIsFirstLoad: bool,
    pub m_unRemainingPackets: u32,
    pub m_unCount: u32,
    pub m_rgAppsUpdated: [AppId_t; 64],
    pub m_unAppsAdded: u64,
}

impl AppLicensesChanged_t {
    pub const K_ICALLBACK: i32 = 1_020_094;
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;
    use core::mem::offset_of;

    #[test]
    fn encrypted_app_ticket_response_layout() {
        assert_eq!(size_of::<EncryptedAppTicketResponse_t>(), 4);
        assert_eq!(EncryptedAppTicketResponse_t::K_ICALLBACK, 154);
    }

    #[test]
    fn app_licenses_changed_layout() {
        // The C++ header comments the exact offsets. Assert them.
        assert_eq!(offset_of!(AppLicensesChanged_t, m_bReloadAll), 0x00);
        assert_eq!(offset_of!(AppLicensesChanged_t, m_bIsFirstLoad), 0x01);
        assert_eq!(offset_of!(AppLicensesChanged_t, m_unRemainingPackets), 0x04);
        assert_eq!(offset_of!(AppLicensesChanged_t, m_unCount), 0x08);
        assert_eq!(offset_of!(AppLicensesChanged_t, m_rgAppsUpdated), 0x0C);
        assert_eq!(offset_of!(AppLicensesChanged_t, m_unAppsAdded), 0x110);
        assert_eq!(size_of::<AppLicensesChanged_t>(), 0x118);
        assert_eq!(AppLicensesChanged_t::K_ICALLBACK, 1_020_094);
    }
}
