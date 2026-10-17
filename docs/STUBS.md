# Stub Tracker

Single source of truth for "what is fake." Update when a stub becomes
real or a new one is introduced.

As of Batch 09b, every code path in the port executes its real
implementation. The only remaining items are a small set of
intentional behavioural divergences from the C++, and a set of C++
sources that were deliberately left unported.

---

## Cross-module bridges -- `src/bridge.rs`

`bridge.rs` exists to break dependency cycles between early batches and
later ones: it lets a module in batch N call into a function that lands
in batch N+1 without the call site having to change. Every entry in
this table now delegates to its real implementation.

| Function | Delegates to |
|---|---|
| `manifest_set_provider` | `utils::steam_metadata::manifest_client::set_provider` |
| `manifest_active_provider_name` | `utils::steam_metadata::manifest_client::active_provider_name` |
| `stats_fetch_stat_steam_id` | `utils::steam_metadata::stats_client::fetch_stat_steam_id` |
| `appticket_write_ownership_ticket` | `utils::tickets::app_ticket::write_app_ownership_ticket` |
| `appticket_write_encrypted_ticket` | `utils::tickets::app_ticket::write_encrypted_ticket` |
| `hooks_notify_license_changed` | `hook::package::notify_license_changed` |
| `hooks_probe_manifest` | `hook::netpacket::probe_manifest` |
| `hooks_get_owned_depots` | `hook::package::get_owned_depots` |
| `hooks_lookup_depot` | `hook::manifest::lookup_depot` |
| `hooks_request_manifest_code` | `hook::netpacket::request_manifest_code` |
| `hooks_get_cache_app_ownership_ticket` | `hook::decryption::get_cache_app_ownership_ticket` |
| `pipe_manager_on_handshake` | `pipe::pipe_manager::on_handshake` |
| `denuvo_auth_is_authorized_pipe` | `pipe::denuvo_auth::is_authorized_pipe` |
| `cloud_sync_app_set` | `utils::cloud_redirect::host::sync_app_set` |

The table is retained as a map of what `bridge.rs` routes where,
because the file is otherwise silent about it. If a future batch
introduces a new stub, add a row.

---

## Deliberately not ported

These were in the C++ codebase but are being skipped for the initial
Rust port. They are all optional or non-critical-path. Revisit if the
scope changes.

| C++ source | Why skipped |
|---|---|
| `Utils/Update/AppUpdater.cpp` | Self-updater for the injected DLL. Fully guarded by `#ifdef OST_ENABLE_UPDATER` in the C++ and never on the critical path for hook functionality. Would need to redeclare `OPENSTEAMTOOL_VERSION` at Rust build time. |
| `Utils/CloudRedirect/...` self-updater pieces | Same reasoning as `AppUpdater`. |
| `Utils/Tokeer/...` clipboard interaction | The C++ `TokeerUri` comment mentions clipboard work but the implementation only uses `ShellExecuteA` plus registry. Nothing to skip. |

---

## Known behavioural divergences

Not stubs -- the code path executes -- but a feature is intentionally
incomplete compared to the C++. These are the items to revisit if the
scope changes.

| Site | Behaviour | Why |
|---|---|---|
| `hook::steamui::BuildCompleteAppOverviewChange` | Installs and passes through. The C++ re-injects removed `removed_appid` entries into the freshly-rebuilt overview so a full library rebuild does not resurrect a removed game. | The C++ calls Steam's protobuf `RepeatedField<uint32>::Add` helper against a message created by the C++ protobuf runtime. prost uses a different in-memory layout for `repeated uint32`, so the same helper cannot be called against prost types. Fix path: resolve `RepeatedFieldUint32_Add` via PatternLoader and call it from Rust with a pointer to whatever Steam hands us. |
| `hook::misc` `GetOrAddAppData` | Not installed. | The install line is commented out in the C++, and the C++ body has a `// TODO: find a more robust way` note. Preserved as a comment in the Rust file. |
| `hook::keyvalues` `install` | Not called from the framework orchestrator. | `HookManager.cpp` calls `Hooks_KeyValues::Install()` in a commented line; the Rust `framework.rs` matches. The module itself is real and works if called. |
| `steam::ipc_facades` | Hand-written, not generated. | The C++ build generates `IPCMessages.gen.h` from `Steam/IPCMessages.steamd` via a small tool. Only `IClientUser` and `IClientUtils` are used by the hook layer, so the Rust side hand-writes just those. A `.steamd`-driven generator would be over-engineering for the current scope. |

---

## How to audit

Search for the strings `stub`, `not ported`, `deferred`, `TODO`, and
`FIXME` across `src/`. Anything a reader finds should be either in one
of the tables above or a false positive (a word used in a comment).
