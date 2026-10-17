//! Byte-layout facades mirroring `IPCMessages.gen.h`.
//!
//! C++ source: generated from `Steam/IPCMessages.steamd`. The C++ file
//! provides one class per IPC method, each wrapping a `CUtlBuffer*` and
//! exposing typed accessors for the request or response body. This is a
//! hand-maintained port of that generated file — the `.steamd` remains
//! the source of truth, and any change to the schema must be reflected
//! here in lockstep.
//!
//! ## Layouts
//!
//! Request:   `[u8 command] [u8 ifaceID] [u32 hSteamUser] [u32 funcHash] [body...] [u32 fencepost]`
//! Response:  `[u8 result]  [body...]`
//! Handshake: `[u8 command] [u32 version] [u32 pid] [u32 tid]`
//!
//! Per-method facades read/write fields at fixed offsets in `body`,
//! matching the offsets the C++ generator emits. Envelope slicing and
//! bounds checking are `BufView`'s job.

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use crate::steam::ipc_messages::{EIPCCommand, EIPCInterface};
use crate::steam::structs::CUtlBuffer;
use crate::steam::structs::CSteamID;
use crate::steam::types::SteamAPICall_t;

// ── Buffer view ─────────────────────────────────────────────────────

/// A view over the valid prefix of a `CUtlBuffer`, or over a caller-owned
/// byte range. Reads and writes are unaligned, matching the C++
/// `detail::Read` / `detail::Write` helpers.
#[derive(Copy, Clone)]
pub struct BufView {
    base: *mut u8,
    len: usize,
}

impl BufView {
    pub const fn null() -> Self {
        Self { base: std::ptr::null_mut(), len: 0 }
    }

    pub fn from_raw(base: *mut u8, len: usize) -> Self {
        Self { base, len }
    }

    /// Build a view over a `CUtlBuffer`'s valid prefix. Matches the C++
    /// `detail::BufferBytes`: a null buffer or a negative `m_Put` yields
    /// an empty view.
    ///
    /// # Safety
    /// `buf`, if non-null, must point to a live `CUtlBuffer`.
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self {
        if buf.is_null() { return Self::null(); }
        let b = &*buf;
        if b.m_Memory.m_pMemory.is_null() || b.m_Put < 0 { return Self::null(); }
        Self { base: b.m_Memory.m_pMemory, len: b.m_Put as usize }
    }

    pub fn is_null(&self) -> bool { self.base.is_null() }
    pub fn is_empty(&self) -> bool { self.base.is_null() || self.len == 0 }
    pub fn len(&self) -> usize { self.len }

    pub fn fits(&self, size: usize) -> bool {
        !self.base.is_null() && self.len >= size
    }

    pub fn slice(&self, offset: usize, size: usize) -> BufView {
        if self.base.is_null() || offset > self.len || size > self.len - offset {
            return BufView::null();
        }
        BufView { base: unsafe { self.base.add(offset) }, len: size }
    }

    /// # Safety
    /// `offset + size_of::<T>() <= self.len`.
    pub unsafe fn read<T: Copy>(&self, offset: usize) -> T {
        debug_assert!(!self.base.is_null());
        debug_assert!(offset <= self.len && std::mem::size_of::<T>() <= self.len - offset);
        std::ptr::read_unaligned(self.base.add(offset) as *const T)
    }

    pub unsafe fn read_u8  (&self, o: usize) -> u8  { self.read(o) }
    pub unsafe fn read_u32 (&self, o: usize) -> u32 { self.read(o) }
    pub unsafe fn read_i32 (&self, o: usize) -> i32 { self.read(o) }
    pub unsafe fn read_u64 (&self, o: usize) -> u64 { self.read(o) }
    pub unsafe fn read_bool(&self, o: usize) -> bool { self.read::<u8>(o) != 0 }

    /// # Safety
    /// Same as `read`. Returns `false` if the range is out of bounds.
    pub unsafe fn write<T: Copy>(&self, offset: usize, v: T) -> bool {
        if self.base.is_null() { return false; }
        if offset > self.len || std::mem::size_of::<T>() > self.len - offset { return false; }
        std::ptr::write_unaligned(self.base.add(offset) as *mut T, v);
        true
    }

    pub unsafe fn write_u8  (&self, o: usize, v: u8 ) -> bool { self.write(o, v) }
    pub unsafe fn write_u32 (&self, o: usize, v: u32) -> bool { self.write(o, v) }
    pub unsafe fn write_i32 (&self, o: usize, v: i32) -> bool { self.write(o, v) }
    pub unsafe fn write_u64 (&self, o: usize, v: u64) -> bool { self.write(o, v) }
    pub unsafe fn write_bool(&self, o: usize, v: bool) -> bool { self.write::<u8>(o, v as u8) }

    /// Copy `src` into `[offset, offset+capacity)`, zero-filling any
    /// remainder. Mirrors `detail::CopyBytes`.
    ///
    /// # Safety
    /// The view must be non-null; the range must be in-bounds for write.
    pub unsafe fn copy_bytes(&self, offset: usize, capacity: usize, src: &[u8]) -> bool {
        if self.base.is_null() { return false; }
        if offset > self.len || capacity > self.len - offset { return false; }
        let dst = self.base.add(offset);
        let n = capacity.min(src.len());
        if n > 0 { std::ptr::copy_nonoverlapping(src.as_ptr(), dst, n); }
        if n < capacity { std::ptr::write_bytes(dst.add(n), 0u8, capacity - n); }
        true
    }
}

// ── Envelopes ───────────────────────────────────────────────────────

/// A request buffer. Layout: `[u8 command][body...]`.
#[derive(Copy, Clone)]
pub struct IPCRequest { bytes: BufView }

impl IPCRequest {
    /// # Safety
    /// `buf` must point at a live `CUtlBuffer`.
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self {
        Self { bytes: BufView::from_cutl(buf) }
    }
    pub fn from_view(bytes: BufView) -> Self { Self { bytes } }
    pub fn ok(&self) -> bool { self.bytes.fits(1) }
    pub fn command_raw(&self) -> u8 {
        if !self.ok() { return 0; }
        unsafe { self.bytes.read_u8(0) }
    }
    pub fn body(&self) -> BufView {
        if self.bytes.len() < 1 { BufView::null() }
        else { self.bytes.slice(1, self.bytes.len() - 1) }
    }
}

/// Body of an `InterfaceCall` request, starting after the `IPCRequest`
/// command byte. Layout: `[u8 ifaceID] [u32 hSteamUser] [u32 funcHash] [body...] [u32 fencepost]`.
#[derive(Copy, Clone)]
pub struct IPCInterfaceCall { bytes: BufView }

impl IPCInterfaceCall {
    pub fn from_body(bytes: BufView) -> Self { Self { bytes } }
    /// Requires at least the 9-byte header plus the 4-byte fencepost.
    pub fn ok(&self) -> bool { self.bytes.fits(13) }
    pub fn interface_id_raw(&self) -> u8 { unsafe { self.bytes.read_u8(0) } }
    pub fn h_steam_user(&self) -> u32 { unsafe { self.bytes.read_u32(1) } }
    pub fn func_hash(&self) -> u32 { unsafe { self.bytes.read_u32(5) } }
    pub fn fencepost(&self) -> u32 {
        if self.bytes.len() < 4 { return 0; }
        unsafe { self.bytes.read_u32(self.bytes.len() - 4) }
    }
    pub fn body(&self) -> BufView {
        if self.bytes.len() < 13 { return BufView::null(); }
        self.bytes.slice(9, self.bytes.len() - 13)
    }
}

/// Handshake request body. Layout: `[u32 version] [u32 pid] [u32 tid]`.
#[derive(Copy, Clone)]
pub struct IPCHandshakeReq { bytes: BufView }

impl IPCHandshakeReq {
    pub fn from_body(bytes: BufView) -> Self { Self { bytes } }
    pub fn ok(&self) -> bool { self.bytes.fits(12) }
    pub fn version(&self) -> u32 { unsafe { self.bytes.read_u32(0) } }
    pub fn pid(&self) -> u32 { unsafe { self.bytes.read_u32(4) } }
    pub fn tid(&self) -> u32 { unsafe { self.bytes.read_u32(8) } }
}

/// A response buffer. Layout: `[u8 result][body...]`.
#[derive(Copy, Clone)]
pub struct IPCResponse { bytes: BufView }

impl IPCResponse {
    /// # Safety
    /// `buf` must point at a live `CUtlBuffer`.
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self {
        Self { bytes: BufView::from_cutl(buf) }
    }
    pub fn from_view(bytes: BufView) -> Self { Self { bytes } }
    pub fn ok(&self) -> bool { self.bytes.fits(1) }
    pub fn result_raw(&self) -> u8 {
        if !self.ok() { return 0; }
        unsafe { self.bytes.read_u8(0) }
    }
    pub fn body(&self) -> BufView {
        if self.bytes.len() < 1 { BufView::null() }
        else { self.bytes.slice(1, self.bytes.len() - 1) }
    }
}

// ── IClientUser ─────────────────────────────────────────────────────

const K_ICLIENT_USER: u8 = EIPCInterface::IClientUser as u8;

/// `IClientUser::GetSteamID() -> CSteamID`.
pub struct GetSteamIDReq { r: IPCRequest }
impl GetSteamIDReq {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCRequest::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCRequest::from_view(v) } }
    fn call(&self) -> IPCInterfaceCall { IPCInterfaceCall::from_body(self.r.body()) }
    pub fn ok(&self) -> bool {
        let c = self.call();
        self.r.ok() && self.r.command_raw() == EIPCCommand::InterfaceCall as u8
            && c.ok() && c.interface_id_raw() == K_ICLIENT_USER
    }
}
pub struct GetSteamIDResp { r: IPCResponse }
impl GetSteamIDResp {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCResponse::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCResponse::from_view(v) } }
    pub fn ok(&self) -> bool { self.r.ok() && self.r.body().fits(8) }
    pub fn return_value(&self) -> CSteamID {
        CSteamID::from_u64(unsafe { self.r.body().read_u64(0) })
    }
    pub fn set_return_value(&self, v: CSteamID) -> bool {
        unsafe { self.r.body().write_u64(0, v.m_unAll64Bits) }
    }
}

/// `IClientUser::GetAppOwnershipTicketExtendedData(unAppID, cbMaxTicket, ...)`.
pub struct GetAppOwnershipTicketExtendedDataReq { r: IPCRequest }
impl GetAppOwnershipTicketExtendedDataReq {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCRequest::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCRequest::from_view(v) } }
    fn call(&self) -> IPCInterfaceCall { IPCInterfaceCall::from_body(self.r.body()) }
    fn body(&self) -> BufView { self.call().body() }
    pub fn ok(&self) -> bool {
        let c = self.call();
        self.r.ok() && self.r.command_raw() == EIPCCommand::InterfaceCall as u8
            && c.ok() && c.interface_id_raw() == K_ICLIENT_USER && c.body().fits(8)
    }
    pub fn un_app_id(&self) -> u32 { unsafe { self.body().read_u32(0) } }
    pub fn cb_max_ticket(&self) -> i32 { unsafe { self.body().read_i32(4) } }
}
pub struct GetAppOwnershipTicketExtendedDataResp {
    r: IPCResponse,
    cb_max_ticket: usize,
}
impl GetAppOwnershipTicketExtendedDataResp {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer, cb_max_ticket: usize) -> Self {
        Self { r: IPCResponse::from_cutl(buf), cb_max_ticket }
    }
    pub fn from_view(v: BufView, cb_max_ticket: usize) -> Self {
        Self { r: IPCResponse::from_view(v), cb_max_ticket }
    }
    pub fn ok(&self) -> bool {
        // returnValue(4) + pTicket(cbMax) + 4*4 = 20 + cbMax
        self.r.ok() && self.r.body().fits(20 + self.cb_max_ticket)
    }
    pub fn return_value(&self) -> u32 { unsafe { self.r.body().read_u32(0) } }
    pub fn set_return_value(&self, v: u32) -> bool { unsafe { self.r.body().write_u32(0, v) } }
    pub fn set_p_ticket(&self, value: &[u8]) -> bool {
        unsafe { self.r.body().copy_bytes(4, self.cb_max_ticket, value) }
    }
    pub fn set_pi_app_id(&self, v: u32) -> bool {
        unsafe { self.r.body().write_u32(4 + self.cb_max_ticket, v) }
    }
    pub fn set_pi_steam_id(&self, v: u32) -> bool {
        unsafe { self.r.body().write_u32(8 + self.cb_max_ticket, v) }
    }
    pub fn set_pi_signature(&self, v: u32) -> bool {
        unsafe { self.r.body().write_u32(12 + self.cb_max_ticket, v) }
    }
    pub fn set_pcb_signature(&self, v: u32) -> bool {
        unsafe { self.r.body().write_u32(16 + self.cb_max_ticket, v) }
    }
}

/// `IClientUser::RequestEncryptedAppTicket(cbData, pData[cbData]) -> SteamAPICall_t`.
pub struct RequestEncryptedAppTicketReq { r: IPCRequest }
impl RequestEncryptedAppTicketReq {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCRequest::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCRequest::from_view(v) } }
    fn call(&self) -> IPCInterfaceCall { IPCInterfaceCall::from_body(self.r.body()) }
    fn body(&self) -> BufView { self.call().body() }
    pub fn ok(&self) -> bool {
        let c = self.call();
        if !(self.r.ok() && self.r.command_raw() == EIPCCommand::InterfaceCall as u8
             && c.ok() && c.interface_id_raw() == K_ICLIENT_USER && c.body().fits(4)) {
            return false;
        }
        let cb = unsafe { self.body().read_i32(0) };
        if cb < 0 { return false; }
        c.body().fits(4 + cb as usize)
    }
    pub fn p_data(&self) -> Vec<u8> {
        let cb = unsafe { self.body().read_i32(0) } as usize;
        let b = self.body().slice(4, cb);
        if b.is_null() { return Vec::new(); }
        unsafe { std::slice::from_raw_parts(b.base, b.len).to_vec() }
    }
}
pub struct RequestEncryptedAppTicketResp { r: IPCResponse }
impl RequestEncryptedAppTicketResp {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCResponse::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCResponse::from_view(v) } }
    pub fn ok(&self) -> bool { self.r.ok() && self.r.body().fits(8) }
    pub fn return_value(&self) -> SteamAPICall_t { unsafe { self.r.body().read_u64(0) } }
}

/// `IClientUser::GetEncryptedAppTicket(cbMaxTicket, pcbTicket, pTicket[pcbTicket])`.
pub struct GetEncryptedAppTicketReq { r: IPCRequest }
impl GetEncryptedAppTicketReq {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCRequest::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCRequest::from_view(v) } }
    fn call(&self) -> IPCInterfaceCall { IPCInterfaceCall::from_body(self.r.body()) }
    pub fn ok(&self) -> bool {
        let c = self.call();
        self.r.ok() && self.r.command_raw() == EIPCCommand::InterfaceCall as u8
            && c.ok() && c.interface_id_raw() == K_ICLIENT_USER && c.body().fits(4)
    }
    pub fn cb_max_ticket(&self) -> u32 { unsafe { self.call().body().read_u32(0) } }
}
pub struct GetEncryptedAppTicketResp { r: IPCResponse }
impl GetEncryptedAppTicketResp {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCResponse::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCResponse::from_view(v) } }
    pub fn ok(&self) -> bool { self.r.ok() && self.r.body().fits(5) }
    pub fn return_value(&self) -> bool { unsafe { self.r.body().read_bool(0) } }
    pub fn set_return_value(&self, v: bool) -> bool { unsafe { self.r.body().write_bool(0, v) } }
    pub fn set_pcb_ticket(&self, v: u32) -> bool { unsafe { self.r.body().write_u32(1, v) } }
    pub fn set_p_ticket(&self, value: &[u8]) -> bool {
        let pcb = unsafe { self.r.body().read_u32(1) };
        unsafe { self.r.body().copy_bytes(5, pcb as usize, value) }
    }
}

// ── IClientUtils ────────────────────────────────────────────────────

const K_ICLIENT_UTILS: u8 = EIPCInterface::IClientUtils as u8;

/// `IClientUtils::GetAppID() -> AppId_t`.
pub struct GetAppIDReq { r: IPCRequest }
impl GetAppIDReq {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCRequest::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCRequest::from_view(v) } }
    fn call(&self) -> IPCInterfaceCall { IPCInterfaceCall::from_body(self.r.body()) }
    pub fn ok(&self) -> bool {
        let c = self.call();
        self.r.ok() && self.r.command_raw() == EIPCCommand::InterfaceCall as u8
            && c.ok() && c.interface_id_raw() == K_ICLIENT_UTILS
    }
}
pub struct GetAppIDResp { r: IPCResponse }
impl GetAppIDResp {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCResponse::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCResponse::from_view(v) } }
    pub fn ok(&self) -> bool { self.r.ok() && self.r.body().fits(4) }
    pub fn return_value(&self) -> u32 { unsafe { self.r.body().read_u32(0) } }
    pub fn set_return_value(&self, v: u32) -> bool { unsafe { self.r.body().write_u32(0, v) } }
}

/// `IClientUtils::GetAPICallResult(hSteamAPICall, cubCallback, iCallbackExpected, pCallback, pbFailed)`.
pub struct GetAPICallResultReq { r: IPCRequest }
impl GetAPICallResultReq {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer) -> Self { Self { r: IPCRequest::from_cutl(buf) } }
    pub fn from_view(v: BufView) -> Self { Self { r: IPCRequest::from_view(v) } }
    fn call(&self) -> IPCInterfaceCall { IPCInterfaceCall::from_body(self.r.body()) }
    fn body(&self) -> BufView { self.call().body() }
    pub fn ok(&self) -> bool {
        let c = self.call();
        self.r.ok() && self.r.command_raw() == EIPCCommand::InterfaceCall as u8
            && c.ok() && c.interface_id_raw() == K_ICLIENT_UTILS && c.body().fits(16)
    }
    pub fn h_steam_api_call(&self) -> u64 { unsafe { self.body().read_u64(0) } }
    pub fn cub_callback(&self) -> u32 { unsafe { self.body().read_u32(8) } }
    pub fn i_callback_expected(&self) -> u32 { unsafe { self.body().read_u32(12) } }
}
pub struct GetAPICallResultResp { r: IPCResponse, cub_callback: usize }
impl GetAPICallResultResp {
    pub unsafe fn from_cutl(buf: *mut CUtlBuffer, cub_callback: usize) -> Self {
        Self { r: IPCResponse::from_cutl(buf), cub_callback }
    }
    pub fn from_view(v: BufView, cub_callback: usize) -> Self {
        Self { r: IPCResponse::from_view(v), cub_callback }
    }
    pub fn ok(&self) -> bool {
        self.r.ok() && self.r.body().fits(2 + self.cub_callback)
    }
    pub fn return_value(&self) -> bool { unsafe { self.r.body().read_bool(0) } }
    pub fn set_return_value(&self, v: bool) -> bool { unsafe { self.r.body().write_bool(0, v) } }
    pub fn set_p_callback(&self, value: &[u8]) -> bool {
        unsafe { self.r.body().copy_bytes(1, self.cub_callback, value) }
    }
    pub fn set_pb_failed(&self, v: bool) -> bool {
        unsafe { self.r.body().write_bool(1 + self.cub_callback, v) }
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buf_view_null_is_empty() {
        let v = BufView::null();
        assert!(v.is_null());
        assert!(v.is_empty());
        assert!(!v.fits(1));
    }

    #[test]
    fn buf_view_read_write_u32() {
        let mut data = [0u8; 32];
        let v = BufView::from_raw(data.as_mut_ptr(), 16);
        unsafe {
            assert!(v.write_u32(0, 0xDEAD_BEEF));
            assert_eq!(v.read_u32(0), 0xDEAD_BEEF);
            assert!(v.write_u32(12, 0x1234_5678));
            assert_eq!(v.read_u32(12), 0x1234_5678);
            // Cannot write 4 bytes starting at offset 13 (13+4 > 16).
            assert!(!v.write_u32(13, 0));
        }
    }

    #[test]
    fn buf_view_slice_and_copy_bytes() {
        let mut data = [0u8; 32];
        let v = BufView::from_raw(data.as_mut_ptr(), 32);
        let sub = v.slice(4, 8);
        assert_eq!(sub.len(), 8);
        unsafe {
            assert!(sub.copy_bytes(0, 4, &[0xAA, 0xBB, 0xCC, 0xDD]));
        }
        assert_eq!(data[4], 0xAA);
        assert_eq!(data[7], 0xDD);
        // Zero-fill the remainder when src is shorter than capacity.
        unsafe {
            assert!(sub.copy_bytes(4, 4, &[0x11]));
        }
        assert_eq!(data[8], 0x11);
        assert_eq!(data[9], 0);
        assert_eq!(data[11], 0);
    }

    #[test]
    fn buf_view_slice_out_of_bounds_is_null() {
        let mut data = [0u8; 8];
        let v = BufView::from_raw(data.as_mut_ptr(), 8);
        assert!(v.slice(6, 4).is_null());
        assert!(v.slice(10, 0).is_null());
    }

    #[test]
    fn ipc_request_command_raw() {
        let mut data = [EIPCCommand::InterfaceCall as u8, 0, 0, 0];
        let v = BufView::from_raw(data.as_mut_ptr(), 4);
        let req = IPCRequest::from_view(v);
        assert!(req.ok());
        assert_eq!(req.command_raw(), EIPCCommand::InterfaceCall as u8);
    }

    #[test]
    fn ipc_request_short_buffer_not_ok() {
        let req = IPCRequest::from_view(BufView::null());
        assert!(!req.ok());
    }

    #[test]
    fn ipc_interface_call_body_offsets() {
        // Body: 9-byte header + 8-byte payload + 4-byte fencepost = 21 bytes.
        let mut data = vec![0u8; 21];
        data[0] = 1; // iface = IClientUser
        data[1..5].copy_from_slice(&0x11223344u32.to_le_bytes());
        data[5..9].copy_from_slice(&0xAABBCCDDu32.to_le_bytes());
        for i in 9..17 { data[i] = i as u8; }
        data[17..21].copy_from_slice(&0xCAFEBABEu32.to_le_bytes());

        let v = BufView::from_raw(data.as_mut_ptr(), 21);
        let call = IPCInterfaceCall::from_body(v);
        assert!(call.ok());
        assert_eq!(call.interface_id_raw(), 1);
        assert_eq!(call.h_steam_user(), 0x11223344);
        assert_eq!(call.func_hash(), 0xAABBCCDD);
        assert_eq!(call.fencepost(), 0xCAFEBABE);
        let b = call.body();
        assert_eq!(b.len(), 8);
        unsafe {
            assert_eq!(b.read_u8(0), 9);
            assert_eq!(b.read_u8(7), 16);
        }
    }

    #[test]
    fn ipc_interface_call_too_short() {
        let mut data = [0u8; 12];
        let call = IPCInterfaceCall::from_body(BufView::from_raw(data.as_mut_ptr(), 12));
        assert!(!call.ok());
    }

    #[test]
    fn ipchandshake_req_layout() {
        let mut data = [0u8; 12];
        data[0..4].copy_from_slice(&7u32.to_le_bytes());
        data[4..8].copy_from_slice(&1234u32.to_le_bytes());
        data[8..12].copy_from_slice(&5678u32.to_le_bytes());
        let hs = IPCHandshakeReq::from_body(BufView::from_raw(data.as_mut_ptr(), 12));
        assert!(hs.ok());
        assert_eq!(hs.version(), 7);
        assert_eq!(hs.pid(), 1234);
        assert_eq!(hs.tid(), 5678);
    }

    #[test]
    fn get_app_id_resp_round_trip() {
        let mut data = [0u8; 8];
        // data[0] is the result byte (unused here).
        let v = BufView::from_raw(data.as_mut_ptr(), 8);
        let resp = GetAppIDResp::from_view(v);
        assert!(resp.ok());
        assert!(resp.set_return_value(730));
        assert_eq!(resp.return_value(), 730);
        // returnValue lives at body offset 0 = buffer offset 1.
        assert_eq!(data[1..5], 730u32.to_le_bytes());
    }

    #[test]
    fn get_steam_id_resp_round_trip() {
        let mut data = [0u8; 12];
        let v = BufView::from_raw(data.as_mut_ptr(), 12);
        let resp = GetSteamIDResp::from_view(v);
        assert!(resp.ok());
        let id = CSteamID::from_u64(0x1100_0010_0000_0001);
        assert!(resp.set_return_value(id));
        assert_eq!(resp.return_value().m_unAll64Bits, 0x1100_0010_0000_0001);
    }

    #[test]
    fn get_api_call_result_resp_padding() {
        let cub_callback = 154usize;
        let total = 1 + 1 + cub_callback + 1;
        let mut data = vec![0u8; total];
        let v = BufView::from_raw(data.as_mut_ptr(), total);
        let resp = GetAPICallResultResp::from_view(v, cub_callback);
        assert!(resp.ok());
        assert!(resp.set_p_callback(&[1, 2, 3]));
        // Callback bytes at body offset 1 = buffer offset 2.
        assert_eq!(data[2], 1);
        assert_eq!(data[3], 2);
        assert_eq!(data[4], 3);
        // Padded with zeros.
        assert_eq!(data[2 + cub_callback - 1], 0);
        assert!(resp.set_pb_failed(true));
        assert_eq!(data[2 + cub_callback], 1);
        // Toggling returnValue at body offset 0 = buffer offset 1.
        assert!(resp.set_return_value(true));
        assert_eq!(data[1], 1);
    }

    #[test]
    fn get_app_ownership_ticket_extended_data_resp_layout() {
        let cb_max = 16usize;
        let total = 1 + 4 + cb_max + 16;
        let mut data = vec![0u8; total];
        let v = BufView::from_raw(data.as_mut_ptr(), total);
        let resp = GetAppOwnershipTicketExtendedDataResp::from_view(v, cb_max);
        assert!(resp.ok());
        assert!(resp.set_return_value(0xDEAD_BEEF));
        assert!(resp.set_p_ticket(&[0xAA; 4]));
        assert!(resp.set_pi_app_id(0x1111_1111));
        assert!(resp.set_pi_steam_id(0x2222_2222));
        assert!(resp.set_pi_signature(0x3333_3333));
        assert!(resp.set_pcb_signature(0x4444_4444));
        // returnValue at body offset 0 = buffer offset 1.
        assert_eq!(&data[1..5], &0xDEAD_BEEFu32.to_le_bytes());
        // pTicket at body offset 4 = buffer offset 5.
        assert_eq!(&data[5..9], &[0xAA; 4]);
        // piAppId at buffer offset 1 + 4 + cb_max.
        assert_eq!(&data[1 + 4 + cb_max .. 1 + 8 + cb_max], &0x1111_1111u32.to_le_bytes());
    }
}
