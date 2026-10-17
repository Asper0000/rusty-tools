# Unused: Denuvo backend

Files here were part of the Denuvo ticket pipeline. They are preserved
for reference but not compiled. `cargo build` will not touch this
directory.

## Original locations

| File | Was at |
|---|---|
| `pipe/denuvo_auth.rs` | `src/pipe/denuvo_auth.rs` |
| `pipe/protection_scan.rs` | `src/pipe/protection_scan.rs` |
| `tickets/app_ticket.rs` | `src/utils/tickets/app_ticket.rs` |
| `tickets/eticket_client.rs` | `src/utils/tickets/eticket_client.rs` |
| `hook/ipc_isteamuser.rs` | `src/hook/ipc_isteamuser.rs` |
| `hook/pending_api_calls.rs` | `src/hook/pending_api_calls.rs` |
| `hook/netpacket/eticket.rs` | `src/hook/netpacket/eticket.rs` |
| `hook/netpacket/ownershipticket.rs` | `src/hook/netpacket/ownershipticket.rs` |

## Why removed

The Denuvo ticket pipeline requires an owning Steam account to mint
signed tickets. Without that, it can only serve the forge path
(works pre-2021 Denuvo) or cached tickets (requires ownership).
Modern Denuvo rejects both.

See CHANGELOG discussion: modern Denuvo (2024+) rejects forged
tickets and requires nonce-bound real tickets from an owning
account.

## Restoring this code

1. Move each file back to its original location (see table above).
2. Re-add the module declarations:
   - `src/pipe/mod.rs`: `pub mod denuvo_auth;`, `pub mod protection_scan;`
   - `src/utils/tickets/mod.rs`: `pub mod app_ticket;`, `pub mod eticket_client;`
   - `src/hook/mod.rs`: `pub mod pending_api_calls;`, `pub mod ipc_isteamuser;`
   - `src/hook/netpacket/mod.rs`: `pub mod eticket;`, `pub mod ownershipticket;`
3. Restore calls:
   - `src/pipe/pipe_manager.rs`: `use crate::pipe::denuvo_auth;` + `denuvo_auth::apply(&ctx);`
   - `src/hook/ipc.rs`: `crate::hook::ipc_isteamuser::register();`
4. Restore ticket handlers in `src/hook/ipc_isteamutils.rs`.
5. Restore ticket readback in `src/hook/decryption.rs`.
6. Restore `src/bridge.rs` bridges (see git history).
7. Restore ownership-ticket-response case in `src/hook/netpacket/dispatch.rs`.

## What still works without these files

- Non-Denuvo unowned games (addappid, DLC, shared depots)
- Owned Denuvo games (they work natively)
- Achievements (`setstat`, stats API)
- Online fix (`-onlinefix`, `-realappid`) Ã¢â‚¬â€ GetAppID spoof still active
- Hot reload
- Manifest pre-seed, MRP, archive
- Cloud redirect (with `cloud_redirect.dll`)
- DLL injection
- URI scheme + Tokeer redemption (registry writes still happen,
  but nothing reads them)
- Legacy CD keys

## What does not work

- Unowned Denuvo games
- `setappticket` / `seteticket` in Lua (no-ops; log warns)
- `forcedenuvo` in Lua (accepted, no consumer)

## Additional items moved here

- cloud_redirect/ and hook/netpacket/cloud.rs - The CloudRedirect backend requires a third-party DLL (cloud_redirect.dll) which was removed. A stub host.rs remains in src/utils/cloud_redirect/ so callers compile.