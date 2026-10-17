//! `Hooks_NetPacket_FamilySharing`. Clears the body of
//! `FamilyGroupsClient.NotifyRunningApps#1` and
//! `CMsgClientSharedLibraryStopPlaying` (eMsg 9406). Both would
//! otherwise trigger the family-share UI path on a non-shared account.

use super::dispatch::RecvReplacement;

pub fn clear_body(_body: &[u8]) -> RecvReplacement {
    LOG_NETPACKET_DEBUG!("Clearing family sharing message...");
    RecvReplacement::Body(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_body_returns_empty_body() {
        match clear_body(&[1, 2, 3]) {
            RecvReplacement::Body(v) => assert!(v.is_empty()),
            _ => panic!("expected Body"),
        }
    }
}
