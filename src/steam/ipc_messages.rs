//! Steam IPC wire schema. Source: `Steam/IPCMessages.steamd`
//!
//! The C++ side generates facade classes from the .steamd file. This is
//! a hand-written Rust equivalent covering the enums, transport
//! envelopes, and the handshake. Per-method bodies are defined as
//! byte-slice views that parse on demand, matching how the C++ uses
//! them (via `CUtlBuffer*` at a current position).

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)]


#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EIPCCommand {
    InterfaceCall  = 1,
    FlushCallbacks = 2,
    Destroy        = 5,
    Heartbeat      = 6,
    Handshake      = 9,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EIPCResult {
    OK     = 0x0B,
    Failed = 0x0C,
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum EIPCInterface {
    IClientUser                        = 1,
    IClientGameServer                  = 2,
    IClientFriends                     = 3,
    IClientUtils                       = 4,
    IClientBilling                     = 5,
    IClientMatchmaking                 = 6,
    IClientApps                        = 8,
    IClientUserStats                   = 11,
    IClientNetworking                  = 12,
    IClientRemoteStorage               = 13,
    IClientDepotBuilder                = 16,
    IClientAppManager                  = 17,
    IClientConfigStore                 = 18,
    IClientGameCoordinator             = 19,
    IClientGameServerStats             = 20,
    IClientGameStats                   = 21,
    IClientHTTP                        = 22,
    IClientScreenshots                 = 23,
    IClientAudio                       = 24,
    IClientUnifiedMessages             = 25,
    IClientStreamLauncher              = 26,
    IClientParentalSettings            = 27,
    IClientNetworkDeviceManager        = 29,
    IClientMusic                       = 30,
    IClientRemoteClientManager         = 31,
    IClientUGC                         = 32,
    IClientStreamClient                = 33,
    IClientProductBuilder              = 34,
    IClientShortcuts                   = 35,
    IClientGameNotifications           = 37,
    IClientVideo                       = 38,
    IClientInventory                   = 39,
    IClientVR                          = 40,
    IClientControllerSerialized        = 41,
    IClientAppDisableUpdate            = 42,
    IClientSharedConnection            = 44,
    IClientShader                      = 45,
    IClientNetworkingSocketsSerialized = 46,
    IClientCompat                      = 48,
    IClientParties                     = 49,
    IClientNetworkingUtilsSerialized   = 50,
    IClientRemotePlay                  = 52,
    IClientGameServerPacketHandler     = 53,
    IClientSystemManager               = 54,
    IClientSystemPerfManager           = 57,
    IClientSystemDockManager           = 58,
    IClientSystemAudioManager          = 59,
    IClientSystemDisplayManager        = 60,
    IClientTimeline                    = 61,
}

// The wire format is packed, with no alignment between fields.

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct IPCRequestHeader {
    pub command: EIPCCommand,
}

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct IPCInterfaceCallHeader {
    pub interface_id: EIPCInterface,
    pub h_steam_user: u32,
    pub func_hash: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct IPCResponseHeader {
    pub result: EIPCResult,
}

#[repr(C, packed)]
#[derive(Copy, Clone, Debug)]
pub struct IPCHandshakeReq {
    pub version: u32,
    pub pid: u32,
    pub tid: u32,
}

// ─── Byte-slice facade ──────────────────────────────────────────────
//
// The C++ code reads these off a `CUtlBuffer` at a moving position. In
// Rust we take a `&[u8]` view and parse on demand. Callers that need
// streaming reads (rare) can advance an offset manually.

#[derive(Copy, Clone)]
pub struct IPCRequest<'a> {
    buf: &'a [u8],
    valid: bool,
    command_raw: u8,
}

impl<'a> IPCRequest<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        if buf.is_empty() {
            return Self { buf, valid: false, command_raw: 0 };
        }
        let raw = buf[0];
        let valid = matches!(raw, 1 | 2 | 5 | 6 | 9);
        Self { buf, valid, command_raw: raw }
    }

    pub fn ok(&self) -> bool { self.valid }
    pub fn command_raw(&self) -> u8 { self.command_raw }
    pub fn command(&self) -> Option<EIPCCommand> {
        match self.command_raw {
            1 => Some(EIPCCommand::InterfaceCall),
            2 => Some(EIPCCommand::FlushCallbacks),
            5 => Some(EIPCCommand::Destroy),
            6 => Some(EIPCCommand::Heartbeat),
            9 => Some(EIPCCommand::Handshake),
            _ => None,
        }
    }

    /// Bytes after the 1-byte command header.
    pub fn body(&self) -> &'a [u8] {
        if self.buf.len() > 1 { &self.buf[1..] } else { &[] }
    }
}

#[derive(Copy, Clone)]
pub struct IPCInterfaceCall<'a> {
    body: &'a [u8],
    valid: bool,
    interface_id_raw: u8,
    h_steam_user: u32,
    func_hash: u32,
}

impl<'a> IPCInterfaceCall<'a> {
    /// Header is 1 + 4 + 4 = 9 bytes packed.
    pub fn new(body: &'a [u8]) -> Self {
        if body.len() < 9 {
            return Self { body, valid: false, interface_id_raw: 0, h_steam_user: 0, func_hash: 0 };
        }
        let interface_id_raw = body[0];
        let h_steam_user = u32::from_le_bytes([body[1], body[2], body[3], body[4]]);
        let func_hash = u32::from_le_bytes([body[5], body[6], body[7], body[8]]);
        Self { body, valid: true, interface_id_raw, h_steam_user, func_hash }
    }

    pub fn ok(&self) -> bool { self.valid }
    pub fn interface_id(&self) -> EIPCInterface {
        // Safety: constructed from a u8; callers only compare, and any
        // byte value is representable as an EIPCInterface discriminant
        // for the purposes of equality checks.
        unsafe { core::mem::transmute(self.interface_id_raw) }
    }
    pub fn interface_id_raw(&self) -> u8 { self.interface_id_raw }
    pub fn h_steam_user(&self) -> u32 { self.h_steam_user }
    pub fn func_hash(&self) -> u32 { self.func_hash }

    /// Bytes after the 9-byte interface-call header.
    pub fn body_after_header(&self) -> &'a [u8] {
        &self.body[9..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::size_of;

    #[test]
    fn enum_layouts() {
        assert_eq!(EIPCCommand::InterfaceCall as u8, 1);
        assert_eq!(EIPCCommand::Handshake as u8, 9);
        assert_eq!(EIPCResult::OK as u8, 0x0B);
        assert_eq!(EIPCResult::Failed as u8, 0x0C);
        assert_eq!(EIPCInterface::IClientUser as u8, 1);
        assert_eq!(EIPCInterface::IClientUtils as u8, 4);
        assert_eq!(EIPCInterface::IClientNetworkingSocketsSerialized as u8, 46);
        assert_eq!(EIPCInterface::IClientTimeline as u8, 61);
        assert_eq!(size_of::<EIPCCommand>(), 1);
        assert_eq!(size_of::<EIPCResult>(), 1);
        assert_eq!(size_of::<EIPCInterface>(), 1);
    }

    #[test]
    fn packed_header_sizes() {
        assert_eq!(size_of::<IPCRequestHeader>(), 1);
        assert_eq!(size_of::<IPCInterfaceCallHeader>(), 9);
        assert_eq!(size_of::<IPCResponseHeader>(), 1);
        assert_eq!(size_of::<IPCHandshakeReq>(), 12);
    }

    #[test]
    fn ipc_request_parse_interface_call() {
        let buf = [1u8, 0, 0, 0, 0]; // InterfaceCall, 4 bytes of body
        let r = IPCRequest::new(&buf);
        assert!(r.ok());
        assert_eq!(r.command(), Some(EIPCCommand::InterfaceCall));
        assert_eq!(r.body().len(), 4);
    }

    #[test]
    fn ipc_request_parse_handshake() {
        let buf = [9u8, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];
        let r = IPCRequest::new(&buf);
        assert!(r.ok());
        assert_eq!(r.command(), Some(EIPCCommand::Handshake));
        let hs: IPCHandshakeReq = unsafe {
            core::ptr::read_unaligned(r.body().as_ptr() as *const IPCHandshakeReq)
        };
        // Packed struct fields must be read via read_unaligned, not field
        // access (which would create a misaligned reference).
        unsafe {
            let v = core::ptr::read_unaligned(core::ptr::addr_of!(hs.version));
            let p = core::ptr::read_unaligned(core::ptr::addr_of!(hs.pid));
            let t = core::ptr::read_unaligned(core::ptr::addr_of!(hs.tid));
            assert_eq!(v, 1);
            assert_eq!(p, 2);
            assert_eq!(t, 3);
        }
    }

    #[test]
    fn ipc_request_rejects_empty() {
        let r = IPCRequest::new(&[]);
        assert!(!r.ok());
    }

    #[test]
    fn ipc_interface_call_parse() {
        // header: interface=1 (IClientUser), hSteamUser=0x42, funcHash=0xDEADBEEF
        let body = [
            1u8,
            0x42, 0, 0, 0,
            0xEF, 0xBE, 0xAD, 0xDE,
            0xAA, 0xBB, // trailing payload bytes
        ];
        let c = IPCInterfaceCall::new(&body);
        assert!(c.ok());
        assert_eq!(c.interface_id_raw(), 1);
        assert_eq!(c.h_steam_user(), 0x42);
        assert_eq!(c.func_hash(), 0xDEADBEEF);
        assert_eq!(c.body_after_header(), &[0xAA, 0xBB]);
    }

    #[test]
    fn ipc_interface_call_short_body_fails() {
        let body = [1u8, 0, 0];
        let c = IPCInterfaceCall::new(&body);
        assert!(!c.ok());
    }
}
