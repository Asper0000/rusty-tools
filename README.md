# rusty_tools

A Rust port of a Windows Steam-IPC toolkit, produced as a **strictly
educational exercise** in code translation, systems programming, and
Windows internals study.

The port preserves the semantics of the original C++ at every layer
that touches the wire format or the ABI; everything else is translated
idiomatically. The translation targets Windows x64 only and links
against the vendored Microsoft Detours static library.

---

## Important - read first

This project is:

- **Educational only.** It exists to study how Windows process
  injection, Detours-style hooking, Steam's IPC envelope format, and
  depot manifest handling work at the systems level.
- **Not for distribution.** Do not publish binaries, release archives,
  or source dumps anywhere. Do not package it as a product. Do not
  present it as a fork, replacement, or successor to any other tool.
- **Not for use on software you do not own.** Interacting with Steam's
  client in the ways this code demonstrates is governed by the Steam
  Subscriber Agreement. Any use against games, accounts, or content you
  do not have the right to inspect is outside the scope of this project
  and is the sole responsibility of whoever does it.

If you are not studying the code for its own sake, this repository is
not for you.

---

## What it is

A translation of a C++ codebase into Rust. The C++ source uses:

- Microsoft Detours for function hooking
- Custom IPC envelopes between the client and `steamclient64.dll`
- Protobuf messages for Steam's service-method calls
- A Lua-based configuration system
- Windows DLL hijacking for injection

The Rust port preserves the semantics of each layer while replacing the
mechanics with Rust-idiomatic equivalents where it does not affect the
wire format or the ABI.

| C++ mechanism | Rust equivalent |
|---|---|
| spdlog | `tracing` + `tracing-subscriber` |
| tomlplusplus | `toml` + `toml_edit` |
| Lua 5.5 | `mlua` (lua54 vendored) |
| Protobuf | `prost` + `prost-build` |
| WinHTTP | `windows-sys` WinHTTP bindings |
| Microsoft Detours | FFI to the vendored Detours static library |
| Win32 API | `windows-sys` |
| CMake codegen | `build.rs` + `prost-build` + hand-written `ipc_facades.rs` |

The library talks to the network through `platform::http`, which uses
WinHTTP directly. `reqwest` and `tokio` remain in `Cargo.toml` for the
CLI's future use but are not on any library hot path.

---

## Layout

    src/
      lib.rs           crate root; includes build.rs's log macros
      main.rs          ost-cli entry point
      dllmain.rs       DllMain + init thread
      bridge.rs        cross-module indirections
      platform/        Win32 abstraction layer
        byte_search.rs
        detour.rs
        dialog.rs
        directory_watch.rs
        dynamic_library.rs
        encoding.rs
        hash.rs
        http.rs
        logging.rs
        memory.rs
        numbers.rs
        pe.rs
        process.rs
        remote_process.rs
        steam_credential_store.rs
        stopwatch.rs
        thread.rs
        trap.rs
        windows/
          detours_ffi.rs
          handles.rs
          nt_abi.rs
      steam/           Steam types, IPC schema, protobuf
        callback.rs
        enums.rs
        ipc_facades.rs
        ipc_messages.rs
        net_packet.rs
        proto.rs
        structs.rs
        types.rs
      hook/            Detours hook layer
        macros.rs           hook_fn! / resolve_fn! / capture_this_fn!
        veh_common.rs       int3 site registry
        framework.rs        transaction helpers + install orchestrators
        pending_api_calls.rs
        callback.rs
        keyvalues.rs
        misc.rs
        steamui.rs
        decryption.rs
        package.rs
        manifest.rs
        ipc.rs
        ipc_isteamuser.rs
        ipc_isteamutils.rs
        netpacket/          one file per NetPacket sub-namespace
      pipe/            pipe handshake, Denuvo detection, injection
        denuvo_auth.rs
        injection.rs
        pipe_manager.rs
        pipe_types.rs
        process_inspector.rs
        protection_scan.rs
      utils/           utilities
        config/
        logging/
        steam_metadata/
        support/
        tickets/
        tokeer/
        cloud_redirect/
    proto/
      steam_messages.proto
    dwmapi_proxy/      loader-hijack proxy producing dwmapi.dll
    xinput1_4_proxy/   loader-hijack proxy producing xinput1_4.dll
    third_party/
      Detours/         vendored Microsoft Detours (build prerequisite)
    docs/
      TESTING.md       manual verification checklist

---

## Build

### Requirements

- Windows 10 or 11, x64
- Visual Studio 2022 with the MSVC x64 toolchain
- Rust 1.75 or newer (`rustup`)
- `third_party/Detours` populated with the Microsoft Detours static
  library:
  - `third_party/Detours/include/detours.h`
  - `third_party/Detours/lib.X64/detours.lib`

  From an x64 Native Tools Command Prompt:

      git clone https://github.com/microsoft/Detours.git third_party\Detours
      cd third_party\Detours\src
      set DETOURS_TARGET_PROCESSOR=X64
      nmake

  `build.rs` fails the build with instructions if either file is
  missing.

### Quick build

From a VS developer prompt, at the repo root:

    scripts\scripts\compile.bat release

or directly:

    cargo build --workspace --release
    cargo test --lib --workspace

### Output

    target\release\RustyTools.dll        the injected hook DLL
    target\release\dwmapi.dll            loader-hijack proxy
    target\release\xinput1_4.dll         loader-hijack proxy
    target\release\ost-cli.exe           developer CLI

---

## Deployment

Copy the three DLLs into the Steam root directory (next to `steam.exe`):

    copy /Y target\release\RustyTools.dll "C:\Program Files (x86)\Steam\"
    copy /Y target\release\dwmapi.dll     "C:\Program Files (x86)\Steam\"
    copy /Y target\release\xinput1_4.dll  "C:\Program Files (x86)\Steam\"

Steam loads `dwmapi.dll` and `xinput1_4.dll` in preference to the
system copies because the application directory precedes the system
directory in the default search order. Each proxy's `DllMain` loads
`RustyTools.dll` when -- and only when -- the host is `steam.exe`, so
nothing runs in other hosts that happen to load those DLLs.

`RustyTools.dll` then runs its own init sequence on a worker thread:
logging, config, pattern + IPC metadata, Lua parse, both file
watchers, hook install for both `steamui.dll` and `steamclient64.dll`,
and finally CloudRedirect, the manifest donor, and the `bst://` URI
scheme registration.

See `docs/TESTING.md` for a step-by-step verification checklist.

---

## Configuration

`opensteamtool.toml`, placed next to `steam.exe`. If absent, built-in
defaults are used. The file is watched while Steam is running; valid
changes are hot-reloaded without restarting Steam.

Key sections:

| Section | Purpose |
|---|---|
| `[log]` | Per-module log level (`trace`..`error`) |
| `[manifest]` | Upstream API for depot manifest request codes; timeouts |
| `[stats]` | Enable the achievements stats API |
| `[update]` | Reserved (self-updater not ported) |
| `[lua]` | Additional Lua config directories |
| `[donate]` | Contribute manifest request codes for owned depots |
| `[[inject]]` | Optional DLL injection into game processes |
| `[cloud]` | Optional Steam Cloud save redirection |
| `[remote]` | Optional metadata mirror overrides |

### Lua configuration

Files matching `*.lua` in the merged watch directories are executed by
an `mlua` state. Function names are lowercase-only. Supported:

| Function | Effect |
|---|---|
| `addappid(id [, flag, key_hex])` | Register a depot; optional 64-char AES key |
| `addtoken(appid, token)` | Access token for a protected depot |
| `setlegacycdkey(appid, key)` | Serve a legacy CD key locally |
| `addprocess(appid, "ExeName.exe")` | Bind a process name to an appid |
| `forcedenuvo(appid)` | Bypass the Denuvo heuristic for an app |
| `seteticketurl(url)` | Backend URL for on-demand eticket minting |
| `setmanifestid(depot_id, gid_str)` | Pin a depot's manifest |
| `setappticket(appid, hex)` | Write an AppTicket to the credential store |
| `seteticket(appid, hex)` | Write an ETicket to the credential store |
| `setstat(appid, "steamid")` | SteamID for achievements |
| `http_get(url [, headers])` | Stub: returns `(nil, "HTTP request failed")` |
| `http_post(url, body [, headers])` | Stub: returns `(nil, "HTTP request failed")` |

`fetch_manifest_code(gid)` and `fetch_manifest_code_ex(app_id,
depot_id, gid)` can be defined by scripts and take priority over the
configured upstream for resolving manifest request codes.

---

## CLI

`ost-cli.exe` is a developer tool. It exercises the same library
functions the DLL uses, without requiring injection. It is the
supported way to sanity-check config and Lua parsing on any machine.

    ost-cli version                      print the library version
    ost-cli init [--config PATH]         load a TOML config and print settings
    ost-cli parse-lua PATH               parse a Lua file or directory
    ost-cli parse-toml PATH              same as init, explicit subcommand
    ost-cli sha256 PATH                  lowercase-hex SHA-256 of a file
    ost-cli fnv1a STRING                 FNV-1a 32-bit hash (pattern-loader key)
    ost-cli handle-uri URL               same handler as the TokeerUri export
    ost-cli help                         print usage

---

## Logging

Debug builds write one file per module under
`<Steam>\opensteamtool\`:

| File | Content |
|---|---|
| `main.log` | Init, config loading, Lua parsing, general utilities |
| `ipc.log` | IPC commands, InterfaceCall dispatch, spoofing |
| `netpacket.log` | WebSocket send/recv, eMsg dispatch |
| `manifest.log` | Manifest download, `fetch_manifest_code`, overrides |
| `decryptionkey.log` | Depot decryption key injection |
| `keyvalue.log` | KeyValues patching |
| `misc.log` | Engine pointer capture, appid hints |
| `achievement.log` | UserStats requests/responses, SteamID spoofing |
| `pics.log` | PICS access token injection |
| `package.log` | Package injection, FileWatcher events |
| `onlinefix.log` | OnlineFix (`-onlinefix`/`-realappid`) diagnostics |
| `richpresence.log` | Rich Presence packet construction |
| `steamui.log` | SteamUI hook diagnostics |
| `pipe.log` | Pipe handshakes, Denuvo auth, injection |
| `platform.log` | Platform helper diagnostics |

Log files are truncated at every Steam start.

---

## Status

Every layer of the original C++ has a Rust counterpart. See
`CHANGELOG.md` for the porting progress and `STUBS.md` for the small
set of deliberate divergences.

Ported layers:

- Platform (Win32 abstraction): encoding, numbers, logging, hash,
  stopwatch, thread, byte search, process, PE, HTTP, directory watch,
  credential store, dialog, trap (VEH), Detours FFI, RAII handles.
- Steam foundation types: `EMsg`, `EResult`, structs, callbacks,
  `CNetPacket` layout probe, IPC message schema.
- Config: TOML, Lua (via `mlua`), file watchers for both.
- Utils: logging, support (FNV, Stopwatch), Steam metadata (pattern
  loader, IPC loader, manifest client, manifest cache, manifest donor,
  mirror chain, remote TOML, stats, diagnostics), tickets (AppTicket,
  ETicket, LegacyCDKey), Tokeer (`bst://` bridge), CloudRedirect.
- Hook: detour macros, VEH, IPC, NetPacket (all sub-namespaces),
  Package, Manifest, Decryption, Misc, SteamUI, Callback, KeyValues.
- Pipe: PipeManager, ProcessInspector, DenuvoAuth, ProtectionScan,
  Injection.
- dllmain: init thread, host gate, detach path.
- Proxy DLLs: `dwmapi.dll`, `xinput1_4.dll`.

Deliberately not ported:

- `Utils/Update/AppUpdater` -- self-updater, `#ifdef OST_ENABLE_UPDATER`
  in the C++ build and off in every shipped configuration. See
  `STUBS.md`.

---

## Testing

- `cargo test --lib --workspace` runs 352 unit tests covering every
  layer that does not require a live Steam process: types, layouts,
  parsers, protocol state machines, Lua loading, TOML schema, IPC
  facades, byte search, PE parsing, hashing, and the log facade.
- `docs/TESTING.md` is a manual checklist for the code paths that
  cannot be unit-tested: loader hijacking, live IPC interception,
  manifest pre-seed, `bst://` registration, Denuvo detection, and
  CloudRedirect hosting.

---

## License

See `LICENSE`.
