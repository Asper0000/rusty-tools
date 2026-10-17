//! Hook layer. Ported from `src/Hook/`.
//!
//! * [`macros`]            - the `hook_fn!` / `install_hook!` family
//! * [`veh_common`]        - shared int3 site registry
//! * [`framework`]         - detour transaction helpers + orchestrators
//! * [`pending_api_calls`] - `SteamAPICall_t -> AppId` map
//! * [`callback`]          - `SendCallbackToPipe`
//! * [`keyvalues`]         - `KeyValues::ReadAsBinary`
//! * [`misc`]              - OnlineFix / SpawnProcess / GetAppID wrap
//! * [`steamui`]           - SteamUI library-visibility hooks
//! * [`decryption`]        - ConfigStore key serve + cached appticket read
//! * [`package`]           - fake license injection + owned-depot list
//! * [`manifest`]          - depot<->app<->gid table + manifest pre-seed
//! * [`ipc`]               - `IPCProcessMessage` hook + dispatch table
//! * [`ipc_isteamuser`]    - per-method handlers for `IClientUser`
//! * [`ipc_isteamutils`]   - per-method handlers for `IClientUtils`
//! * [`netpacket`]         - eMsg dispatch on the WebSocket send/recv pair

pub mod macros;

pub mod veh_common;
pub mod framework;
pub mod callback;
pub mod keyvalues;

pub mod misc;
pub mod steamui;
pub mod decryption;

pub mod package;
pub mod manifest;

pub mod ipc;
pub mod ipc_isteamutils;

pub mod netpacket;
