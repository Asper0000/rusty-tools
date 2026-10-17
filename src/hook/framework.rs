//! Detour transaction helpers and the module-level install
//! orchestrators that mirror `HookManager.cpp`.
//!
//! Sub-batches grow this file as modules land:
//!   6a: callback
//!   6b: decryption, misc, steamui
//!   6c: manifest, package
//!   6d: ipc
//!   6e-1: netpacket

use crate::platform::detour;

pub struct Transaction { begun: bool }

impl Transaction {
    pub fn begin() -> Self { Self { begun: detour::begin_transaction() } }

    pub fn commit(self, all_ok: bool) -> bool {
        if !self.begun { return false; }
        let c = detour::commit_transaction();
        std::mem::forget(self);
        c && all_ok
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if self.begun { detour::abort_transaction(); }
    }
}

pub fn install_steamclient_hooks() {
    let txn = Transaction::begin();
    let mut ok = true;
    ok &= crate::hook::callback::install();
    ok &= crate::hook::decryption::install();
    ok &= crate::hook::ipc::install();
    ok &= crate::hook::manifest::install();
    ok &= crate::hook::misc::install();
    ok &= crate::hook::netpacket::install();
    ok &= crate::hook::package::install();
    let _ = txn.commit(ok);
}

pub fn uninstall_steamclient_hooks() {
    let txn = Transaction::begin();
    let mut ok = true;
    ok &= crate::hook::callback::uninstall();
    ok &= crate::hook::decryption::uninstall();
    ok &= crate::hook::ipc::uninstall();
    ok &= crate::hook::manifest::uninstall();
    ok &= crate::hook::misc::uninstall();
    ok &= crate::hook::netpacket::uninstall();
    ok &= crate::hook::package::uninstall();
    let _ = txn.commit(ok);
    crate::hook::veh_common::disarm_all();
    crate::hook::veh_common::remove_handler();
}

pub fn install_steamui_hooks() {
    let txn = Transaction::begin();
    let mut ok = true;
    ok &= crate::hook::steamui::install();
    let _ = txn.commit(ok);
}

pub fn uninstall_steamui_hooks() {
    let txn = Transaction::begin();
    let mut ok = true;
    ok &= crate::hook::steamui::uninstall();
    let _ = txn.commit(ok);
}
