//! IPC method metadata loader. Source: `IPCLoader.cpp`.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::platform::numbers;
use super::remote_toml::{self, Request as RtRequest};
use super::steam_diagnostics;

#[derive(Clone, Debug)]
pub struct Method {
    pub interface_id: u8,
    pub name: String,
    pub func_hash: u32,
    pub fencepost: u32,
    pub argc: u32,
}

#[derive(Clone, Debug)]
pub struct Interface {
    pub id: u8,
    pub name: String,
    pub vtable_rva: u32,
    pub methods: Vec<Method>,
}

struct Registry {
    interfaces: Vec<Interface>,
    by_id: HashMap<u8, usize>,
    by_name: HashMap<String, usize>,
}

impl Registry {
    fn new() -> Self {
        Self { interfaces: Vec::new(), by_id: HashMap::new(), by_name: HashMap::new() }
    }
    fn add(&mut self, iface: Interface) {
        let idx = self.interfaces.len();
        self.by_id.insert(iface.id, idx);
        self.by_name.insert(iface.name.clone(), idx);
        self.interfaces.push(iface);
    }
    fn find_by_hash(&self, id: u8, func_hash: u32) -> Option<&Method> {
        let idx = *self.by_id.get(&id)?;
        self.interfaces[idx].methods.iter().find(|m| m.func_hash == func_hash)
    }
    fn find_by_name(&self, iface: &str, method: &str) -> Option<&Method> {
        let idx = *self.by_name.get(iface)?;
        self.interfaces[idx].methods.iter().find(|m| m.name == method)
    }
    fn method_count(&self) -> usize {
        self.interfaces.iter().map(|i| i.methods.len()).sum()
    }
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn with_registry<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    let mut g = REGISTRY.lock().expect("poisoned");
    if g.is_none() { *g = Some(Registry::new()); }
    f(g.as_mut().unwrap())
}

/// Maps EIPCInterface name → numeric ID. Port of `EIPCInterfaceFromName`.
/// This covers only the interfaces declared in `Steam/IPCMessages.steamd`.
fn eipc_from_name(name: &str) -> Option<u8> {
    Some(match name {
        "IClientUser" => 1,
        "IClientGameServer" => 2,
        "IClientFriends" => 3,
        "IClientUtils" => 4,
        "IClientBilling" => 5,
        "IClientMatchmaking" => 6,
        "IClientApps" => 8,
        "IClientUserStats" => 11,
        "IClientNetworking" => 12,
        "IClientRemoteStorage" => 13,
        "IClientDepotBuilder" => 16,
        "IClientAppManager" => 17,
        "IClientConfigStore" => 18,
        "IClientGameCoordinator" => 19,
        "IClientGameServerStats" => 20,
        "IClientGameStats" => 21,
        "IClientHTTP" => 22,
        "IClientScreenshots" => 23,
        "IClientAudio" => 24,
        "IClientUnifiedMessages" => 25,
        "IClientStreamLauncher" => 26,
        "IClientParentalSettings" => 27,
        "IClientNetworkDeviceManager" => 29,
        "IClientMusic" => 30,
        "IClientRemoteClientManager" => 31,
        "IClientUGC" => 32,
        "IClientStreamClient" => 33,
        "IClientProductBuilder" => 34,
        "IClientShortcuts" => 35,
        "IClientGameNotifications" => 37,
        "IClientVideo" => 38,
        "IClientInventory" => 39,
        "IClientVR" => 40,
        "IClientControllerSerialized" => 41,
        "IClientAppDisableUpdate" => 42,
        "IClientSharedConnection" => 44,
        "IClientShader" => 45,
        "IClientNetworkingSocketsSerialized" => 46,
        "IClientCompat" => 48,
        "IClientParties" => 49,
        "IClientNetworkingUtilsSerialized" => 50,
        "IClientRemotePlay" => 52,
        "IClientGameServerPacketHandler" => 53,
        "IClientSystemManager" => 54,
        "IClientSystemPerfManager" => 57,
        "IClientSystemDockManager" => 58,
        "IClientSystemAudioManager" => 59,
        "IClientSystemDisplayManager" => 60,
        "IClientTimeline" => 61,
        _ => return None,
    })
}

fn parse_interface(name: &str, tbl: &toml::value::Table, out: &mut Interface) -> bool {
    out.name = name.to_string();
    let Some(expected) = eipc_from_name(name) else {
        LOG_WARN!("IPCLoader: [{}] is not declared in EIPCInterface", name);
        return false;
    };
    out.id = expected;

    if let Some(v) = tbl.get("interface_id").and_then(|v| v.as_integer()) {
        if v < 0 || v > 0xFF {
            LOG_WARN!("IPCLoader: [{}] interface_id out of range ({})", name, v);
            return false;
        }
        if v as u8 != out.id {
            LOG_WARN!("IPCLoader: [{}] interface_id {} disagrees with generated value {}",
                      name, v, out.id);
            return false;
        }
    }

    if let Some(v) = tbl.get("vtable_rva").and_then(|v| v.as_str()) {
        if let Some(rva) = numbers::parse_hex_u32(v) { out.vtable_rva = rva; }
    }

    for (k, val) in tbl {
        let Some(mtbl) = val.as_table() else { continue };
        let Some(hash_str) = mtbl.get("funcHash").and_then(|v| v.as_str()) else {
            LOG_WARN!("IPCLoader: [{}.{}] missing funcHash", name, k);
            continue;
        };
        let Some(func_hash) = numbers::parse_hex_u32(hash_str) else {
            LOG_WARN!("IPCLoader: [{}.{}] bad funcHash '{}'", name, k, hash_str);
            continue;
        };
        let mut m = Method {
            interface_id: out.id,
            name: k.clone(),
            func_hash,
            fencepost: 0,
            argc: 0,
        };
        if let Some(v) = mtbl.get("fencepost").and_then(|v| v.as_str()) {
            if let Some(f) = numbers::parse_hex_u32(v) { m.fencepost = f; }
        }
        if let Some(v) = mtbl.get("argc").and_then(|v| v.as_integer()) {
            m.argc = v as u32;
        }
        out.methods.push(m);
    }
    true
}

fn show_missing_popup(sha: &str) {
    steam_diagnostics::show_warning(
        "rusty_tools - IPC spec missing".to_string(),
        format!(
            "rusty_tools: IPC spec file not found.\n\nIPC interception is disabled for this session; pattern-based hooks are unaffected.\n\nYou can:\n  1. Wait for the next upstream publish and restart Steam.\n  2. Drop a matching TOML at:\n       <Steam>\\rustytools\\ipc\\steamclient\\{}.toml\n  3. Check upstream:\n       https://github.com/madoiscool/steam-monitor/tree/ipc/steamclient",
            sha
        ),
    );
}

pub const IPC_CHANNEL: &str = "ipc";

pub fn load(steamclient_path: &str) -> bool {
    with_registry(|r| { r.interfaces.clear(); r.by_id.clear(); r.by_name.clear(); });

    let r = remote_toml::fetch(&RtRequest {
        channel: IPC_CHANNEL.into(),
        component: "steamclient".into(),
        dll_path: steamclient_path.into(),
    });

    if !r.ok {
        let sha = if r.sha256.is_empty() { "(hash failed)" } else { &r.sha256 };
        show_missing_popup(sha);
        return false;
    }

    let parsed: toml::Value = match toml::from_str(&r.body) {
        Ok(v) => v,
        Err(e) => {
            LOG_WARN!("IPCLoader: TOML parse error: {}", e);
            show_missing_popup(&r.sha256);
            return false;
        }
    };
    let Some(root) = parsed.as_table() else {
        show_missing_popup(&r.sha256);
        return false;
    };

    let mut added = 0;
    for (k, v) in root {
        let Some(tbl) = v.as_table() else { continue };
        let mut iface = Interface { id: 0, name: String::new(), vtable_rva: 0, methods: Vec::new() };
        if !parse_interface(k, tbl, &mut iface) { continue; }
        with_registry(|r| r.add(iface));
        added += 1;
    }

    let (mc, ic) = with_registry(|r| (r.method_count(), r.interfaces.len()));
    LOG_INFO!("IPCLoader: loaded {} methods across {} interfaces ({})",
              mc, ic, if added > 0 && !r.from_cache { "remote" } else { "cache fallback" });
    true
}

pub fn find_by_hash(interface_id: u8, func_hash: u32) -> Option<Method> {
    with_registry(|r| r.find_by_hash(interface_id, func_hash).cloned())
}

pub fn find_by_name(iface: &str, method: &str) -> Option<Method> {
    with_registry(|r| r.find_by_name(iface, method).cloned())
}

pub fn interface_count() -> usize { with_registry(|r| r.interfaces.len()) }
pub fn method_count() -> usize { with_registry(|r| r.method_count()) }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eipc_from_name_known_values() {
        assert_eq!(eipc_from_name("IClientUser"), Some(1));
        assert_eq!(eipc_from_name("IClientUtils"), Some(4));
        assert_eq!(eipc_from_name("IClientNetworkingSocketsSerialized"), Some(46));
        assert_eq!(eipc_from_name("Nope"), None);
    }
}
