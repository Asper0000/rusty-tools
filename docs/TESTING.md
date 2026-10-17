# Manual testing checklist

Everything in this file requires a working Steam install and, for most
steps, administrator rights. None of it can run under `cargo test`: the
code paths exercise the actual Steam client, the loader, and the
credential store. It is the closest thing to end-to-end verification the
project has.

Every check below is a "pass / fail / skip". If a check fails, keep the
matching log file from `<Steam>\opensteamtool\` (see the table at the
end) and file it with the build's SHA-256 from `ost-cli sha256`.

## 0. Prerequisites

- Windows 10 or 11, x64.
- Steam installed and logged into an account.
- A Visual Studio 2022 Developer Command Prompt (x64 Native Tools).
- `third_party/Detours` populated, per `CHANGELOG.md` Batch 06a-prep.
- `cargo build --workspace` succeeds from the repo root.

## 1. Build

From the VS developer prompt, at the repo root:

    scripts\scripts\compile.bat release

Expected output ends with:

    Binary: target\release\ost-cli.exe
    DLLs  : target\release\RustyTools.dll, dwmapi.dll, xinput1_4.dll

`cargo test --lib --workspace` should print
`352 passed; 0 failed; 0 ignored`.

## 2. Deploy

Copy the three DLLs into the Steam root directory (next to `steam.exe`):

    copy /Y target\release\RustyTools.dll "C:\Program Files (x86)\Steam\"
    copy /Y target\release\dwmapi.dll     "C:\Program Files (x86)\Steam\"
    copy /Y target\release\xinput1_4.dll  "C:\Program Files (x86)\Steam\"

Optionally drop an `opensteamtool.toml` next to them.

## 3. Start Steam

Fully exit Steam (tray icon -> Exit) and start `steam.exe`.

### What to check

- [ ] Steam starts and logs in normally. The DLLs are loaded at process
      start. If a required pattern file cannot be fetched and the
      fallback cache is empty, a popup will say so and the hooks tied to
      that DLL are disabled for the session, but Steam still starts.
- [ ] `<Steam>\opensteamtool\` now contains one log file per module:
      `main.log`, `ipc.log`, `netpacket.log`, `manifest.log`,
      `decryptionkey.log`, `keyvalue.log`, `misc.log`,
      `achievement.log`, `pics.log`, `package.log`, `onlinefix.log`,
      `richpresence.log`, `steamui.log`, `pipe.log`, `platform.log`.
- [ ] `main.log` ends with `OpenSteamTool init complete`.

### If Steam fails to start

Move the three DLLs out of the Steam root and start Steam again to
recover. The most common cause is the Detours static library not being
present at build time; `build.rs` fails the build in that case, so a
successfully built binary should not exhibit it.

## 4. Pattern files loaded

Open `<Steam>\opensteamtool\main.log` and search for:

    PatternLoader: loaded N patterns for steamui
    PatternLoader: loaded N patterns for steamclient

Both should appear. `N` is the number of entries in the matching
signature TOML. If either is missing, the corresponding pattern file
was not retrieved. A popup will have been shown with the DLL's
SHA-256 and the expected cache path. Fetching the file manually into
`<Steam>\opensteamtool\pattern\<component>\<sha256>.toml` and
restarting Steam is the workaround.

## 5. IPC metadata loaded

In `ipc.log`:

    IPCLoader: loaded N methods across M interfaces

The numbers depend on the upstream IPC spec. If the IPC spec could not
be fetched, `IPC interception is disabled` appears and a popup is
shown; the rest of the tool still works.

## 6. Hooks installed

In `main.log`, search for the hook install sequence:

    Hooks_IPC: N handlers registered

If a specific hook's target could not be resolved, `PatternLoader: no
entry for 'X'` appears in `main.log`, and the missing-functions popup
lists every function that failed across the whole run. That popup
should be empty on a supported Steam build.

## 7. Config hot-reload

With Steam running, edit `<Steam>\opensteamtool.toml` -- for example,
change `[log] level` from `debug` to `info`. Within ~1 second,
`main.log` should show:

    ConfigFileWatcher: ...opensteamtool.toml changed, reloading
    Config loaded: manifest.url=... log.level=info ...

The per-module log files will start filtering at the new level
immediately.

## 8. Lua hot-reload

Drop a `.lua` file into `<Steam>\config\stplug-in\` (or any directory
listed under `[lua].paths`). Within ~1 second, `package.log` should
show:

    Lua file added: <path>
    Lua refresh completed

Add `addappid(730)` to the file and save; `NotifyLicenseChanged` runs
and the library list refreshes. Removing a `.lua` file triggers the
same sequence with `removed`.

## 9. CLI

Without a live Steam process:

    ost-cli.exe version
    ost-cli.exe fnv1a BBuildAndAsyncSendFrame
    ost-cli.exe parse-lua <path-to-directory>
    ost-cli.exe parse-toml <path-to-config>

`fnv1a` must print `0x82428E37`. `parse-lua` and `parse-toml` print
summary lines that mirror the state the DLL would have built.

## 10. bst:// URI handler

If `opensteamtool.toml` is present, the URI scheme is registered on
every Steam startup (`TokeerBridge: registered bst:// scheme` in
`main.log`). Test the handler directly first:

    ost-cli.exe handle-uri bst://redeem/TESTCODE

This calls the same function `rundll32` would, from a normal process.
On the happy path, the server returns tickets and the redeem prompt
appears. On any error (unknown code, server unreachable), a MessageBox
is shown.

The real OS-level test is: paste a `bst://...` URL into a browser
address bar and press Enter. Windows should launch `rundll32.exe` with
the DLL as the handler. This is the definitive check that the HKCU
registration is correct.

## 11. Manifest pre-seed (requires a Lua-added game)

Configure a game via `addappid` that is not owned. Launch it. In
`manifest.log`:

    YldLoadDepotManifest: pre-seed app=... depot=... gid=...
    ManifestCache: cached depot=... gid=... (N bytes) -> <steam>\depotcache\...

If the manifest is already archived, download should start on the
first attempt without a manifest request code being visible in
`netpacket.log`. If the manifest is not archived yet, a single popup
listing the missing depots appears after a brief quiet period.

## 12. -onlinefix

Launch a game via the Steam UI with `-onlinefix` in its launch
options. In `misc.log`:

    SpawnProcess: appid N -> 480, realappid=false

In `onlinefix.log`, after the game opens a P2P connection:

    NetworkingSockets active: GetAppID now reports 480 for cert match

## 13. -realappid

Add `-realappid` to a game already using `-onlinefix`. In `misc.log`
at launch:

    SpawnProcess: appid N -> 480, realappid=true

Every subsequent `GetAppID` answer remains the real appid.

## 14. Denuvo

Two sub-checks, both requiring a real Denuvo-protected game in the
Lua config:

Detection. On game launch, in `pipe.log`:

    DenuvoAuth: success to detect Denuvo in module <path>

The detection method (`OepPattern`, `LegacySectionString`, or
`ProtectedBlobSection`) is logged with it. If nothing is detected but
the game is known-protected, `forcedenuvo(N)` in Lua forces the
branch.

Authorization window. On the first handshake:

    DenuvoAuth: authorization pipe selected ...

and, once the window closes:

    DenuvoAuth: authorization window ended ...
    DenuvoAuth: persisted end-authorization SteamID auth_appid=... steamid=...

## 15. CloudRedirect

Only if `[cloud].enabled = true` and `cloud_redirect.dll` is present
next to `steam.exe`:

    CloudRedirect: loaded <path> and initialised cloud save redirection
    CloudRedirect: registered N redirected app(s)

If the DLL is present but its exports are missing, `CloudRedirect:
cloud_redirect.dll is missing required exports, disabling` appears
instead, and the rest of the tool behaves as if `[cloud]` were unset.

## 16. Shutdown

Close Steam (tray -> Exit). The `DLL_PROCESS_DETACH` path stops both
watchers, unhooks every installed detour, and disarms every VEH site.
DenuvoAuth state is per-process and is not persisted across restarts.

## Log file reference

| File | What to look for on a healthy run |
|---|---|
| `main.log`          | `OpenSteamTool init complete`; `Config loaded: ...`; `PatternLoader: loaded N patterns` |
| `ipc.log`           | `IPCLoader: loaded N methods across M interfaces`; `Hooks_IPC: N handlers registered` |
| `netpacket.log`     | `CNetPacket layout = <name> (m_pubData +0x...)` |
| `manifest.log`      | `GetManifestRequestCode send: ...` when downloading a Lua-added game |
| `decryptionkey.log` | `Providing decryption key for depot N` when a game loads a Lua-configured depot |
| `keyvalue.log`      | (silent unless KV manipulation is enabled) |
| `misc.log`          | `SpawnProcess: appid N -> 480, ...` on `-onlinefix` launches |
| `achievement.log`   | `Player::GetUserStats request: ... appid=N` on stats queries |
| `pics.log`          | `CMsgClientPicsProductInfoRequest: found appid N with access_token` |
| `package.log`       | `LicenseList: N license(s)` at login; `OwnedDepots: done -- N package(s) resolved, M unique` |
| `onlinefix.log`     | `NetworkingSockets active` after an `-onlinefix` game starts P2P |
| `richpresence.log`  | `Tracking topmost appid N` when an unlocked game is launched |
| `steamui.log`       | `FillInAppOverview: set PurchasedTime=N for appId=M` |
| `pipe.log`          | `PipeManager: handshake ...`; `DenuvoAuth: success to detect Denuvo` |
| `platform.log`      | Diagnostics from the platform layer; should be empty on a clean run |

## What is *not* covered here

- Loader hijacking. Placing the DLLs next to a real `steam.exe` and
  starting Steam is the test; there is no automated way to do it
  without a real Steam install.
- Actual DLL injection into a game process (`[[inject]]` entries).
  Exercise it manually with a game whose launch options you control.
- Network access to the upstream APIs. A correctly built DLL will talk
  to them if the network allows; failure is logged, not fatal.
- Credential-store writes. `setAppTicket` and `setETicket` write to
  `HKCU\Software\Valve\Steam\Apps\<appid>`. Verifying them requires
  reading the registry after the fact.

## Reporting a failure

Capture:

- the exact `cargo test --lib --workspace` output
- the DLL SHA-256 from
  `ost-cli.exe sha256 target\release\RustyTools.dll`
- the Steam build ID from `<Steam>\opensteamtool\main.log`
  (search for `SteamDiagnostics: ... build=`)
- the relevant per-module log file(s) from the table above
- the exact command or UI action that triggered the failure
