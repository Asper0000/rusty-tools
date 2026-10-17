//! `PendingAPICalls.cpp`. A map from `SteamAPICall_t` to the `AppId` the
//! call was made for. Populated by an IPC pre-handler and drained by the
//! matching `GetAPICallResult` handler.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::steam::types::{k_uAPICallInvalid, k_uAppIdInvalid, AppId_t, SteamAPICall_t};

static PENDING: Mutex<Option<HashMap<SteamAPICall_t, AppId_t>>> = Mutex::new(None);

fn with_map<R>(f: impl FnOnce(&mut HashMap<SteamAPICall_t, AppId_t>) -> R) -> R {
    let mut g = PENDING.lock().expect("pending-api-calls mutex poisoned");
    if g.is_none() {
        *g = Some(HashMap::new());
    }
    f(g.as_mut().unwrap())
}

pub fn record_encrypted_ticket(call: SteamAPICall_t, app_id: AppId_t) {
    if call == k_uAPICallInvalid || app_id == k_uAppIdInvalid {
        return;
    }
    with_map(|m| { m.insert(call, app_id); });
}

pub fn take_encrypted_ticket(call: SteamAPICall_t) -> Option<AppId_t> {
    with_map(|m| m.remove(&call))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_then_take() {
        record_encrypted_ticket(0x1000, 730);
        assert_eq!(take_encrypted_ticket(0x1000), Some(730));
        // Second take is None.
        assert_eq!(take_encrypted_ticket(0x1000), None);
    }

    #[test]
    fn record_rejects_invalid_call() {
        record_encrypted_ticket(0, 730);
        assert_eq!(take_encrypted_ticket(0), None);
    }

    #[test]
    fn record_rejects_invalid_app_id() {
        record_encrypted_ticket(0x2000, 0);
        assert_eq!(take_encrypted_ticket(0x2000), None);
    }

    #[test]
    fn take_missing_returns_none() {
        assert_eq!(take_encrypted_ticket(0xDEAD_BEEF), None);
    }

    #[test]
    fn record_overwrites_previous() {
        record_encrypted_ticket(0x3000, 100);
        record_encrypted_ticket(0x3000, 200);
        assert_eq!(take_encrypted_ticket(0x3000), Some(200));
    }
}
