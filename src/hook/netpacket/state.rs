//! Global packet state. Mirrors the C++ file-static buffers. Written
//! only from the network thread inside the two hooks; no locking.

use std::cell::UnsafeCell;

pub const K_MAX_BODY_SIZE: usize = 131072;
pub const K_MAX_HDR_SIZE:  usize = 1024;
pub const K_MAX_PACKET_SIZE: usize = 8 + K_MAX_HDR_SIZE + K_MAX_BODY_SIZE;
pub const K_PACKET_POOL_SIZE: usize = 8;

pub struct NetPacketState {
    // incoming
    pub new_body: [u8; K_MAX_BODY_SIZE],
    pub cb_new_body: u32,
    pub new_hdr: [u8; K_MAX_HDR_SIZE],
    pub cb_new_hdr: u32,
    pub need_replace_body: bool,
    pub need_replace_hdr: bool,
    pub resized_in_place: bool,
    pub new_body_size: u32,
    pub recv_packet_pool: [[u8; K_MAX_PACKET_SIZE]; K_PACKET_POOL_SIZE],
    pub recv_packet_pool_idx: usize,

    // outgoing
    pub send_new_body: [u8; K_MAX_BODY_SIZE],
    pub cb_send_new_body: u32,
    pub need_replace_send: bool,
    pub suppress_send: bool,
    pub send_packet_pool: [[u8; K_MAX_PACKET_SIZE]; K_PACKET_POOL_SIZE],
    pub send_packet_pool_idx: usize,

    // layout probe
    pub probe_attempts: i32,
    pub probe_agreed: u32,
    pub probe_logged: bool,
}

impl NetPacketState {
    pub const fn new() -> Self {
        Self {
            new_body: [0u8; K_MAX_BODY_SIZE],
            cb_new_body: 0,
            new_hdr: [0u8; K_MAX_HDR_SIZE],
            cb_new_hdr: 0,
            need_replace_body: false,
            need_replace_hdr: false,
            resized_in_place: false,
            new_body_size: 0,
            recv_packet_pool: [[0u8; K_MAX_PACKET_SIZE]; K_PACKET_POOL_SIZE],
            recv_packet_pool_idx: 0,
            send_new_body: [0u8; K_MAX_BODY_SIZE],
            cb_send_new_body: 0,
            need_replace_send: false,
            suppress_send: false,
            send_packet_pool: [[0u8; K_MAX_PACKET_SIZE]; K_PACKET_POOL_SIZE],
            send_packet_pool_idx: 0,
            probe_attempts: 0,
            probe_agreed: 0,
            probe_logged: false,
        }
    }
}

pub struct GlobalState(UnsafeCell<NetPacketState>);
unsafe impl Sync for GlobalState {}

pub static STATE: GlobalState = GlobalState(UnsafeCell::new(NetPacketState::new()));

/// # Safety
/// Caller must be on the network thread, or must otherwise guarantee no
/// other thread is inside the send/recv hooks.
#[inline]
pub unsafe fn state() -> &'static mut NetPacketState {
    &mut *STATE.0.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_state_starts_zeroed() {
        // Read the process-global static directly. `NetPacketState` is
        // ~1.18 MiB (two 8-entry ring pools of ~65 KiB each, plus
        // scratch buffers); constructing one by value overflows the
        // test harness's 2 MiB thread stack in a debug build. The
        // static lives in .bss and has no such problem.
        //
        // SAFETY: no unit test in this crate mutates the packet state,
        // and the ones that read it take the same accessor under the
        // same single-threaded assumption.
        let s = unsafe { state() };
        assert_eq!(s.probe_attempts, 0);
        assert_eq!(s.recv_packet_pool_idx, 0);
        assert_eq!(s.send_packet_pool_idx, 0);
        assert!(!s.need_replace_body);
        assert!(!s.need_replace_send);
        assert!(!s.suppress_send);
    }
}
