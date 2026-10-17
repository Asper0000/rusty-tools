# Changelog

All notable changes to **rusty_tools** are documented here.

> **Scope and intent** - this project is a **strictly educational** exercise in
> reading and translating an existing C++ codebase into Rust, to study how a
> Windows process works with Steam's IPC, depot, and manifest layers.
>
> **It is not intended for distribution.** No binaries, release archives,
> or source dumps should be shared outside the personal learning environment
> in which this port was produced. It is not a fork, not a redistribution,
> and not a replacement for any upstream tool.
>
> Nothing in this repository should be used to violate the Steam Subscriber
> Agreement, any game's EULA, or any applicable law. Any use against
> software the operator does not own or have the right to inspect is
> explicitly outside the scope of this project.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to a `MAJOR.MINOR.PATCH` scheme tied to porting
progress rather than external API stability.

---

## [Unreleased]

### Batch 10.5 - xinput1_4 eager load in DllMain

**Fixed (suspected):**
- `xinput1_4_proxy/src/lib.rs` - the port loaded the real
  `%WINDIR%\System32\xinput1_4.dll` lazily on first wrapper call
  (Batch 08 design note). This was a deliberate divergence from the
  C++, which loads it eagerly in `DllMain`. The lazy load turns out
  to be the cause of the "Steam dies ~5s after start" symptom on
  this system: `steamwebhelper.exe` (Chromium UI) calls
  `XInputGetState` during UI setup, and the `LoadLibraryW` inside
  our wrapper runs from a context close to the loader, deadlocking
  the UI child. Steam.exe's watchdog then kills the whole process.

  Now: eager load in `DllMain`, matching the C++. `let _ = real();`
  forces the `OnceLock` initialization before `DLL_PROCESS_ATTACH`
  returns.

**Tests:** no net change. 360.

### Batch 10.4 - register_uri_scheme on a detached thread

**Fixed (suspected):**
- `src/dllmain.rs` - `register_uri_scheme` was being called
  synchronously on the init thread. The `[wf]` trace added in Batch
  10.3 showed the init thread reaching `[wf] register_uri_scheme:
  enter` and never logging `[wf] register_uri_scheme: done`, so the
  init thread was blocked inside that function while Steam's own
  watchdog fired. The function writes to
  `HKCU\Software\Classes\bst\shell\open\command`, which is a known
  persistence location that can block on some systems. Do not block
  init on it: `register_uri_scheme` is now spawned on a detached
  thread. The init thread logs
  `[wf] register_uri_scheme: spawning on detached thread` and
  completes normally.

**Added:**
- `src/utils/tokeer/bridge.rs` - granular `[wf]` trace inside
  `register_uri_scheme`. Before/after each of the three `write_key`
  calls, and around `RegCreateKeyExA`. If a future build still
  blocks on the write, the trace names the exact operation.

**Note:**

The initial 10.4 patch failed on `bridge.rs` because the multi-line
`let ok = write_key(...) && ...` anchor didn't match the file's
actual whitespace/line endings. This revision does the same edit
via a line-based search for `let ok = write_key(` and the next
line ending in `&command);`, replacing that range with the three
traced calls. Robust to any formatting variation. If the pattern
is still not found, the patch prints the region around
`let ok = write_key(` so the file state can be inspected directly.

**Tests:** no net change. 360.

### Batch 10.3 - workflow trace in init_thread

**Added (diagnostic only, no behaviour change):**
- `src/dllmain.rs` - `init_thread` now logs a `[wf] <step>: enter` line
  before and a `[wf] <step>: done` line after every major step. Every
  line is written to `main.log` with the usual timestamp. The steps
  covered, in order:
  * `initialize_steam_components`
  * `config::load`
  * `logging::init_modules_public`
  * `steam_diagnostics::initialize`
  * `pattern_loader::load (steamui)`
  * `pattern_loader::load (steamclient)`
  * `ipc_loader::load`
  * `lua parse`
  * `lua_file_watcher::start`
  * `config_file_watcher::start`
  * `install_steamui_hooks`
  * `install_steamclient_hooks`
  * `report_missing_functions`
  * `cloud_redirect::initialize`
  * `manifest_donor::start`
  * `register_uri_scheme`

  This lets a reader see exactly which step was in flight when the
  process died. `[wf] <step>: enter` with no matching `: done` means
  that step hung. `: done` for everything up to step N but nothing for
  step N+1 means the process exited between them.

- `src/dllmain.rs` - `DllMain` now logs `[wf] DllMain
  DLL_PROCESS_ATTACH is_steam_host={}` at the top of the ATTACH
  branch and `[wf] DllMain DLL_PROCESS_DETACH` at the top of the
  DETACH branch. The pairing tells us whether the process exited
  before or after `init_thread` returned:
  * `DLL_PROCESS_DETACH` before `[wf] init_thread: exit` â†’ process
    exit killed the init thread.
  * `[wf] init_thread: exit` before `DLL_PROCESS_DETACH` â†’ init
    completed, Steam died for its own reasons.
  * Neither â†’ neither handler fired (process was killed hard).

**Tests:** no net change. 360.

### Batch 10.2 - remove duplicate FreeLibrary import

**Fixed:**
- `tools/extract_tickets/src/main.rs` - the 10.1 patch added
  `use windows_sys::Win32::Foundation::FreeLibrary;` before the
  `LibraryLoader` import block unconditionally. Running 10.1 twice
  (or, in any future scenario, re-applying it) produced two copies
  of that import, which `rustc` rejects with `E0252: the name
  FreeLibrary is defined multiple times`.

  This batch dedupes the `Foundation::FreeLibrary` import lines,
  keeping exactly one. The insert site in 10.1 was not idempotent;
  this patch fixes the symptom and would need 10.1 itself to be
  rewritten for idempotency if it's ever re-emitted.

**Note on patch idempotency:**

This is the third batch in a row where a re-run produced a
compile error instead of a no-op (08.1 / 08.3 were pattern-misses
that were already handled gracefully, but 10.1's anchor insert
was not). The pattern is worth naming: `ReplaceInFile` with
"warn on miss" makes *replacement* idempotent, but *insertion*
anchored on a substring that the previous run did not consume
is not. Insertion patches need an explicit presence check before
the write.

**Tests:** no net change. 360.

### Batch 10.1 - fix `FreeLibrary` import in extract_tickets

**Fixed:**
- `tools/extract_tickets/src/main.rs` - `FreeLibrary` was imported
  from `windows_sys::Win32::System::LibraryLoader`, which does not
  export it in `windows-sys` 0.59. The linker-loader family is split
  across two modules: `LoadLibraryW` / `LoadLibraryExW` /
  `GetProcAddress` / `GetModuleFileNameW` are in `LibraryLoader`;
  `FreeLibrary` / `FreeLibraryAndExitThread` are in `Foundation`.
  Moved the `FreeLibrary` import to `Win32::Foundation`.

  No `Cargo.toml` change needed: the tool's
  `[target.'cfg(windows)'.dependencies]` already enables
  `Win32_Foundation`.

**Tests:** no net change. 360.

### Batch 10 - rename to rustytools + port tools/extract_tickets

**Fixed:**
- `src/dllmain.rs` - the `bst://` URI scheme registration was
  constructing a path to `<steam>\OpenSteamTool.dll`. The cdylib has
  been named `RustyTools.dll` since Batch 08, so the registration was
  pointing at a file that did not exist. Changed to `RustyTools.dll`.

**Changed (rename to rustytools):**
- Config file: `<Steam>\opensteamtool.toml` -> `<Steam>\rustytools.toml`.
  Same schema, same keys, same defaults; only the filename changes.
  The schema lives in `config\rustytools.example.toml` at the repo root.
- Metadata directory: `<Steam>\opensteamtool\` -> `<Steam>\rustytools\`.
  Covers the per-module logs, the pattern-signature cache
  (`<Steam>\rustytools\pattern\<component>\<sha256>.toml`), and the
  IPC-metadata cache (`<Steam>\rustytools\ipc\steamclient\<sha256>.toml`).
- Popups: titles and message bodies that said "OpenSteamTool" now
  say "rusty_tools"; path references say `<Steam>\rustytools\...`
  instead of `<Steam>\opensteamtool\...`.
- Diagnostic dump in `steam_diagnostics.rs` reads
  `rusty_tools version:` instead of `OpenSteamTool version:`.
- `README.md` and `docs/TESTING.md` updated for the new paths.

Affected source files:
  * `src/dllmain.rs`
  * `src/main.rs`
  * `src/utils/config/config.rs`
  * `src/utils/logging/state.rs`
  * `src/utils/steam_metadata/remote_toml.rs`
  * `src/utils/steam_metadata/pattern_loader.rs`
  * `src/utils/steam_metadata/ipc_loader.rs`
  * `src/utils/steam_metadata/steam_diagnostics.rs`

**Added (config\rustytools.example.toml):**
- Reference config at the repo root for users to copy to
  `<Steam>\rustytools.toml`. Same content as the C++
  `opensteamtool.example.toml`; only the header comment names the new
  filename. Explicitly documents that `http_get`/`http_post` are now
  real WinHTTP calls (fixed in 09c) so scripts can rely on them.

**Added (tools/extract_tickets):**
- `tools/extract_tickets/` - standalone Windows-x64 binary, workspace
  member. Mirrors the C++ tool:
  * Reads `<SteamPath>` from
    `HKCU\Software\Valve\Steam\SteamPath` (32-bit WoW64 view).
  * Loads `steamclient64.dll` with
    `LoadLibraryExW(..., LOAD_WITH_ALTERED_SEARCH_PATH)`.
  * Resolves `CreateInterface`, opens a pipe, connects to the
    logged-in user.
  * Calls `ISteamAppTicket::GetAppOwnershipTicketData` for the
    ownership ticket and
    `ISteamUser::RequestEncryptedAppTicket` + polling
    `IsAPICallCompleted` for the encrypted ticket (15 s cap, 50 ms
    step).
  * Writes `<appid>/appticket.bin`, `<appid>/eticket.bin`, and
    `<appid>/tickets.txt` into the current working directory, matching
    the C++ output layout exactly.
- The tool is included in both `[workspace].members` and
  `default-members`, so `cargo build` builds it alongside the DLLs.
  This is a small divergence from C++'s `EXCLUDE_FROM_ALL`; Rust's
  compile time makes the distinction meaningless, and users expect a
  single `cargo build --workspace` to produce all four artifacts.

**Design notes:**
- **Vtable ABI.** Steamworks is C++: each interface is a pointer to
  a vtable of fn pointers, laid out in declaration order. The Rust
  port models this as `#[repr(C)]` structs with `extern "system"`
  fn-pointer fields. On x64 Windows `extern "system"` is the platform
  ABI, matching C++ `__cdecl` (which is the effective default for
  Steamworks on x64).
- **`bool` at the ABI layer.** C++ `bool` is `u8`. Every `bool`
  parameter and return value in the vtables is `u8`, avoiding any
  assumption about the upper bits of the return register.
- **Pre-flight cleanup.** The initial 10a patch's first `ReplaceInFile`
  looked for `{}\opensteamtool.toml`, but the actual source has
  `{}\\opensteamtool.toml` (two backslashes in the source, one in the
  runtime string). The pattern didn't match, and the 10a run was a
  no-op. Batches 10 is a single combined patch with corrected
  patterns; the user re-runs it once and gets the whole rename plus
  the tool.

**Tests:** 5 new (parse_app_id accept/reject, to_hex_lower round-trip,
ticket_line null, ticket_line hex). Running total: 360.

**The port is now complete against every `.cpp` in the C++ tree**
(except `AppUpdater`, which the user asked to skip).

### Batch 09c.2 - drop stray `&` in headers_table_to_string test

**Fixed:**
- `src/utils/config/lua_config.rs` - the `headers_table_to_string`
  test added in 09c called `headers_table_to_string(&t)`.
  09c.1 changed the function signature to take
  `tbl: mlua::Table<'_>` by value, so the `&` at the call site no
  longer type-checks (`error[E0308]: mismatched types`). Dropped the
  `&`. `mlua::Table` is a cheap reference-counted handle into the Lua
  registry, so the move is free and the test does not use `t` again
  afterward.

**Tests:** no net change. 355.

### Batch 09c.1 - fix do_http lifetimes + dwmapi test-build warning

**Fixed:**
- `src/utils/config/lua_config.rs` - the `do_http` helper inserted in
  09c did not compile, for two reasons:
  * `error[E0106]: missing lifetime specifiers` - the return type
    `mlua::Result<(Value, Value)>` contains a `Value<'lua>`, whose
    lifetime cannot be elided because mlua 0.9's `Value` borrows from
    the `Lua` handle it was created from. The function now takes
    `'lua` explicitly:
    `fn do_http<'lua>(lua: &'lua Lua, ..., headers: Option<mlua::Table<'lua>>)
     -> mlua::Result<(Value<'lua>, Value<'lua>)>`.
  * `error[E0507]: cannot move out of *tbl which is behind a shared
    reference` - `mlua::Table::pairs` takes `self` by value, but
    `headers_table_to_string` took `&mlua::Table`. The function now
    takes `tbl: mlua::Table<'_>` by value, and the caller passes
    `tbl` instead of `&tbl`. `mlua::Table` is a cheap reference-counted
    handle into the Lua registry, so the move is trivial.

- `dwmapi_proxy/src/lib.rs` - the `.drectve` include generated by
  `build.rs` is now gated on `#[cfg(not(test))]`. The `.drectve`
  sections carry `/EXPORT:...` directives; in the cdylib they produce
  the correct forwarders, but in the `cargo test --lib` binary they
  are parsed by LINK as exports on an `.exe`, which MSVC reports as
  `LNK4104: export of symbol 'DllCanUnloadNow' should be PRIVATE`.
  The warning was benign (the test binary does not need an export
  table) but noisy. Gating the include on non-test removes it without
  changing the cdylib output.

**Tests:** no net change. 355 (352 plus the 3 the 09c patch attempted
to add and that now compile).

**Running total:** 355.

### Batch 09c - real Lua HTTP helpers + case-insensitive lookup

**Fixed:**
- `src/utils/config/lua_config.rs` - the two Lua HTTP helpers are now
  real. They call `platform::http::execute` (WinHTTP, with the same
  defaults the C++ passes), and their return shape matches the C++
  contract exactly:
  * `http_get(url [, headers])` -> `(body_string, status_code)`
  * `http_post(url, body [, headers])` -> `(body_string, status_code)`
  * failure in either -> `(nil, "HTTP request failed")`

  `headers` is an optional Lua table of `{["Key"]="Value", ...}`
  serialised to the `"Key: Value\r\n"` form WinHTTP expects, matching
  `LuaHeadersToWstr` in `LuaConfig.cpp`. Non-scalar entries are
  skipped, mirroring `lua_tostring` returning NULL for those types.

  **Impact:** scripts that use the `fetch_manifest_code` pattern
  documented in `opensteamtool.example.toml` -- reaching a custom
  manifest-code endpoint via `http_get` -- now reach their endpoint.
  Previously they fell straight through to the "no response" branch
  and the provider table.

- `src/utils/config/lua_config.rs` - case-insensitive global lookup.
  A metatable is installed on `_G` whose `__index` metamethod
  lowercases the requested name and re-tries the raw lookup.
  `setAppTicket`, `SETAPPTICKET`, `SetAppticket`, and `setappticket`
  are now interchangeable, as the upstream README documents.
  Previously only the exact lowercase spelling resolved, so any
  mixed-case call in an existing config was a silent no-op.

**Changed:**
- `STUBS.md` - the "Lua environment" section is removed. Neither
  divergence remains.

**Design notes:**
- The `__index` metamethod only fires on a *miss*. A lookup for a name
  that is already present in `_G` (the common case) never reaches it,
  so there is no metamethod overhead on the hot path.
- Inside the metamethod, `raw_get` bypasses the metatable, avoiding
  recursion into the same closure.
- If the lowercased form equals the original (already-lowercase, or
  non-ASCII that does not case-fold), the metamethod returns nil
  without a second lookup -- the raw miss has already proven there is
  nothing to find.

**Tests:** 3 new
- `headers_table_to_string_formats_correctly` (fresh Lua state, no
  process-global involvement)
- `case_insensitive_lookup_reaches_registered_functions` (walks
  `_G` for `AddAppid`, `ADDAPPID`, `aDdApPiD`, `addAppId`, and a
  genuinely unknown name)
- `http_get_invalid_url_returns_nil_and_error` (invalid URL fails at
  WinHTTP parse time, so no network is touched)

Running total: 355.

### Batch 09b - documentation pass

**Added:**
- `docs/TESTING.md` - a manual verification checklist for every code
  path that cannot be unit-tested. 16 sections covering build,
  deployment, init sequence, pattern + IPC metadata, hook install,
  config and Lua hot-reload, the CLI, the `bst://` URI handler,
  manifest pre-seed, `-onlinefix`/`-realappid`, Denuvo detection, and
  CloudRedirect hosting, followed by a per-module log reference table
  and a failure-report template. This is the counterpart to the 352
  unit tests, for the parts of the port that only exist once Steam is
  running.

**Changed:**
- `README.md` - full rewrite. The previous file was a "what it is"
  section with an empty `## Layout`. Now covers: build prerequisites
  including the Detours vendoring step, the three DLLs and the CLI in
  the output set, deployment, config schema, Lua function list,
  CLI reference, per-module log table, port status, testing, and
  license.
- `STUBS.md` - full rewrite. The previous version was materially
  wrong: it claimed `directory_watch::Watch::open`, both file
  watchers, `detour::*`, the hook-layer bridges, the pipe bridges,
  and `appticket_write_*` / `cloud_sync_app_set` were still
  stubbed. All of those are real. The new file:
  * keeps the "Deliberately not ported" table (`AppUpdater` and the
    two related items), which is still accurate;
  * keeps the "Lua environment" table (`http_get`/`http_post` stubs,
    no case-insensitive lookup), which is still accurate;
  * keeps the "Known behavioural divergences" table
    (`BuildCompleteAppOverviewChange`, `GetOrAddAppData`,
    `keyvalues::install`, hand-written `ipc_facades`), which is
    still accurate;
  * replaces the dead "Hook, Pipe, dllmain, proto -- not ported"
    section with a factual statement that every code path now
    executes its real implementation;
  * updates the cross-module bridge table from "these are stubs" to
    "these are the delegations `bridge.rs` performs", since the file
    is otherwise silent about what it routes.
- `scripts\compile.bat` - the post-build message now prints a `DLLs` line
  alongside the `Binary` line for both `build` and `release`, so a
  reader sees all four artifacts instead of only the CLI.

**The port is complete.** Every file under `Hook/`, `Pipe/`, `Utils/`,
`Steam/`, `OSTPlatform/`, `dllmain.cpp`, `dwmapi/`, and `xinput1_4/`
has a Rust counterpart. The three remaining items in `STUBS.md` are
intentional divergences that would require C++-runtime protobuf
interop to close, not missed work.

**Tests:** no net change. All 352 still pass.

### Batch 09a - CLI, bridge delegations, dwmapi warning suppression

**Fixed:**
- `dwmapi_proxy/build.rs` - added `cargo:rustc-cdylib-link-arg=/ignore:4104`.
  After Batch 08.3 succeeded, the last link-time output was two
  `LNK4104` warnings on `DllCanUnloadNow` and `DllGetClassObject`:
  MSVC nags that those two exports should be `PRIVATE`. That is true
  for a normal in-process COM DLL; this is a forwarder proxy and the
  full export table has to be visible. The warning is suppressed for
  the dwmapi proxy build only.

**Changed:**
- `src/bridge.rs` - the last three stubs become real delegations:
  * `appticket_write_ownership_ticket` ->
    `utils::tickets::app_ticket::write_app_ownership_ticket`
  * `appticket_write_encrypted_ticket` ->
    `utils::tickets::app_ticket::write_encrypted_ticket`
  * `cloud_sync_app_set` ->
    `utils::cloud_redirect::host::sync_app_set`

  This resolves the last row of the "Cross-module bridges" table in
  `STUBS.md`. Lua's `setAppTicket` and `setETicket` now reach the
  credential-store writers; `LuaFileWatcher`'s post-reload call to
  `CloudRedirectHost::SyncAppSet` now forwards to the real DLL if
  CloudRedirect is loaded.

- `src/main.rs` - replaced the 9-line stub with a real CLI. No C++
  counterpart; this is a developer tool. Subcommands:
  * *(none)* - `RustyTools::init()` and exit.
  * `version` - print `RustyTools::VERSION`.
  * `init [--config PATH]` - run the TOML loader the DLL uses and
    print the resulting settings.
  * `parse-lua PATH` - parse a Lua file or directory and summarise
    what the LuaConfig registry collected (depot count, key count,
    token count, manifest-code callbacks present, eticket URL set).
  * `parse-toml PATH` - same as `init`, explicit subcommand.
  * `sha256 PATH` - lowercase-hex SHA-256 of a file, matching what
    the pattern loader computes for its cache key.
  * `fnv1a STRING` - FNV-1a 32-bit hash in hex and decimal, the
    pattern-loader key format. `ost-cli fnv1a BBuildAndAsyncSendFrame`
    prints `0x82428E37`.
  * `handle-uri URL` - same entry point as the `TokeerUri` export,
    so a `bst://` URL can be exercised without invoking `rundll32`.
  * `help`, `-h`, `--help` - usage.
  * Anything else - error, usage, exit code 2.

  All subcommands are plain `stdout`; none of them require injection.
  `handle-uri` on Windows can pop a modal `MessageBox` on its error
  path, matching the C++ `TokeerUri` behaviour exactly.

**Tests:** no net change. All 352 still pass.

**Remaining:**
- Batch 09b - documentation pass: `docs/TESTING.md`, `README.md`,
  `STUBS.md`, `scripts\compile.bat`.

### Batch 08.3 - dwmapi forwarders via `.drectve` sections

**Fixed:**
- `dwmapi_proxy/build.rs` - the 08.2 build tried to fix the Batch 08
  forwarder link failure by emitting the named rows as
  `cargo:rustc-cdylib-link-arg=/EXPORT:Name=DWMAPI.Name,@Ord`. That
  failed with the same 13 `LNK2001: unresolved external symbol`
  errors as the original `/DEF:` approach. The failing symbols were
  once again `DllCanUnloadNow`, `DllGetClassObject`,
  `DwmTetherTextContact`, and the ten `Dwmp*` internals.

  Cause: the self-reference shortcut is inside `link.exe`'s
  export-table builder, which both the `.def` parser and the
  command-line `/EXPORT:` parser feed into. `Module == output-DLL-name`
  (case-insensitive) is normalised to "this module", rewriting the
  forwarder string to `Name=Name`. Only one export mechanism skips
  that normalisation: an object-file `.drectve` section, which is
  exactly what the C++'s
  `#pragma comment(linker, "/EXPORT:...")` emits.

  `dwmapi_proxy/build.rs` now:
  * parses `dwmapi.def` (unchanged, still the source of truth),
  * writes each named row as a `#[link_section = ".drectve"]`
    `#[used]` static into
    `$OUT_DIR/dwmapi_exports_drectve.rs`,
  * writes the ordinal-only rows into a temporary `.def` in
    `$OUT_DIR` and hands *that* to LINK via `/DEF:` (that form
    already linked cleanly),
  * prints a one-line summary to stderr with the split counts.

- `dwmapi_proxy/src/lib.rs` - added an `include!` of the generated
  `.drectve` file after `#![allow(non_snake_case)]`.

**Design note - why `.drectve` and not `/EXPORT:`:**

Three ways to declare an export exist on MSVC: `__declspec(dllexport)`
(source), an `EXPORTS` statement in a `.def` file, and an `/EXPORT:`
specification on the LINK command line. A fourth mechanism exists but
is rarely named in this context: object-file `.drectve` sections, which
LINK reads as additional options. The C++ `#pragma comment(linker, ...)`
is a source-level spelling of that fourth mechanism.

Only the fourth preserves a forwarder's module name verbatim when the
module name matches the output DLL. The first two go through the
export-table builder's `Module == OutputName` normalisation. This was
verified twice - once by the 08 build (`/DEF:` failed) and once by the
08.2 build (`/EXPORT:` command-line failed) - before the `.drectve`
form was adopted. The test of a successful fix is that the two
`DllCanUnloadNow` / `DllGetClassObject` symbols, which `DWMAPI.lib`
does not re-export, resolve.

**Tests:** no net change. All 352 still pass.

### Batch 08.2 - fix dwmapi forwarder self-reference

**Fixed:**
- `dwmapi_proxy/build.rs` - the 08 build handed `dwmapi.def` to LINK via
  `/DEF:`. MSVC's `.def` parser treats `Name=DWMAPI.Name` as a
  *self-reference* whenever `DWMAPI` (case-insensitively) matches the
  DLL currently being linked. Our output is `dwmapi.dll`, so every
  named forwarder row was rewritten by LINK to `Name=Name` - a
  local-symbol export. The 33 exports that the Windows SDK's
  `DWMAPI.lib` happens to re-export resolved by accident; the 13 that
  it does not (`DllCanUnloadNow`, `DllGetClassObject`,
  `DwmTetherTextContact`, and the ten `Dwmp*` internals) failed with
  `LNK2001: unresolved external symbol`, terminating the build with a
  fatal `LNK1120`.

  The C++ build avoided this because it used
  `#pragma comment(linker, "/EXPORT:Name=DWMAPI.Name,@Ord")`, which
  places the directive on the linker *command line*. LINK's
  command-line `/EXPORT` processing does not take the self-reference
  shortcut, so it emits a proper forwarder. `dwmapi_proxy/build.rs`
  now does the same thing from Rust:
  * it parses `dwmapi.def` (unchanged, still the single source of
    truth for the export table),
  * emits each **named** row as
    `cargo:rustc-cdylib-link-arg=/EXPORT:Name=DWMAPI.Name,@Ord`,
  * writes only the **ordinal-only** rows
    (`#N=DWMAPI.#N @N NONAME`) into a temporary `.def` in `OUT_DIR`
    and hands *that* to LINK via `/DEF:`, since that form already
    linked cleanly.

  Runtime behaviour is unchanged: Windows' loader resolves a forwarder
  to a module name by skipping the DLL that is currently being loaded,
  so `DWMAPI.DllCanUnloadNow` from inside our `dwmapi.dll` resolves to
  the system32 copy. That is the documented behaviour the whole hijack
  relies on.

- `xinput1_4_proxy/src/lib.rs` - added `#![allow(non_camel_case_types)]`.
  The seven `*_t` type aliases for the forwarded XInput function
  pointers mirror the C++ `typedef`s verbatim and are intentionally not
  renamed. Silences seven upper-camel-case warnings that were present
  in every build since 08.

**Note on `patch.bat`:**

The `###FILE_*###` marker pattern writes whole files. That is fine for
files owned by a single batch, but destructive for files that accrete
edits across batches. `Cargo.toml`, `lib.rs`, `bridge.rs`, `STUBS.md`,
and `CHANGELOG.md` should be patched with targeted insert/delete, not
wholesale rewrites - the previous batch silently dropped `paste = \"1\"`
from `Cargo.toml` for exactly this reason (fixed in 08.1). This batch
only rewrites `dwmapi_proxy/build.rs`, which is owned outright by the
dwmapi proxy work, and edits `xinput1_4_proxy/src/lib.rs` in place.

**Tests:** no net change. All 352 still pass.

### Batch 08.1 - restore `paste` dependency

**Fixed:**
- `Cargo.toml` - the Batch 08 patch replaced the whole `[dependencies]`
  table with a snapshot that predated 06a's addition of `paste = "1"`.
  The result was 92 compile errors in the root crate: every
  `::paste::paste!` expansion inside `src/hook/macros.rs` failed with
  `cannot find paste in the crate root`, which in turn broke every
  macro-generated symbol the hook layer relies on (`CAPTURED_*`, `O_*`,
  `o_*`, `hk_*`) - 22 `E0425` cascade errors on top of the 70 `E0433`s.
  `paste = "1"` is restored; the crate compiles and every test passes
  again.

**Changed:**
- `xinput1_4_proxy/src/lib.rs` - added `#![allow(non_camel_case_types)]`
  so the seven `*_t` type aliases used for the forwarded XInput function
  pointers stop emitting upper-camel-case warnings. The alias names
  mirror the C++ `typedef`s verbatim and are intentionally not renamed.

**Why the original patch silently broke:**
- The 08 patch's PowerShell writes whole files from `###FILE_*###`
  markers. That is fine when the source is a fresh file, but
  `Cargo.toml` had been edited by 06a since the patch's author last
  copied it. Whole-file writes on a shared, incrementally-edited file
  do not merge, they overwrite. Future `Cargo.toml` patches should
  edit the existing file (add/remove specific lines) rather than write
  a full replacement.

**Tests:** no net change. All 352 still pass.

### Batch 08 - Loader-hijack proxy DLLs

**Source files ported:** `dwmapi/dwmapi.cpp`, `xinput1_4/xinput1_4.cpp`,
`xinput1_4/xinput1_4.def`.

**Added:**
- `dwmapi_proxy/` - a standalone cdylib producing `dwmapi.dll`. Its
  entire export table is forwarders to the real system DWMAPI; see
  `dwmapi.def` for the 106-row mapping (46 named, 60 ordinal-only
  `NONAME`), which mirrors the `#pragma comment(linker, ...)` block in the
  C++ source line for line. `build.rs` emits `cargo:rustc-cdylib-link-arg=/DEF:`
  and `DllMain` loads `RustyTools.dll` only when the host is `steam.exe`.
- `xinput1_4_proxy/` - a standalone cdylib producing `xinput1_4.dll`. Loads
  the real `%WINDIR%\System32\xinput1_4.dll` lazily on first call, resolves
  the seven named exports plus ordinals 100/101/102/103/104/108, and
  forwards each. `xinput1_4.def` assigns the seven named wrappers to
  their real ordinals and marks the six undocumented ordinals `NONAME`.
  Same `steam.exe` gate in `DllMain`.

**Changed:**
- `Cargo.toml` - converted to a workspace root. `[workspace]` declares
  `dwmapi_proxy` and `xinput1_4_proxy` as members; `default-members`
  includes all three packages so `cargo build` at the root builds the
  hook DLL and both proxies together. The `[lib] name` is renamed from
  `rusty_tools` to `RustyTools`, so the cdylib artifact is
  `RustyTools.dll`, and the proxies load that name at runtime.
- `src/main.rs` - `rusty_tools::init()` becomes `RustyTools::init()`,
  matching the renamed library crate. No behavioural change.

**Design notes:**
- **Why forwarders for dwmapi.** The C++ file exports no functions of its
  own. Every entry is a link-time forwarder to the real module; the loader
  resolves each imported symbol to `DWMAPI.dll` at process start. The Rust
  cdylib has zero `#[no_mangle]` exports and takes the same approach —
  `dwmapi.def` is the entire API surface.
- **Why wrappers for xinput1_4.** The real DLL exposes undocumented
  ordinals (100 = `XInputGetStateEx` reads the guide button, which Steam
  Big Picture depends on). A pass-through forward would work by name but
  the ordinals need to land in our export table at the correct numbers, so
  the Rust side defines real functions and the .def pins them.
- **Lazy load in xinput1_4.** The real DLL is resolved on the first
  wrapper call, not in `DllMain`. Loading it eagerly under the loader lock
  while another hijack DLL is already loading would deadlock on some
  configurations; deferring to first call avoids the re-entrancy entirely.
  A wrapper called before the module resolves returns
  `ERROR_DEVICE_NOT_CONNECTED` (1167), matching the C++'s null-pointer
  branch.
- **Failing DllMain aborts the load.** Both proxies return `FALSE` from
  `DLL_PROCESS_ATTACH` if `LoadLibraryA("RustyTools.dll")` fails on a
  real steam.exe host. That matches the C++ and is the safe behaviour: the
  host either gets both DLLs or neither, rather than silently running
  without the hook layer.
- **Artifact naming.** The C++ shipped `OpenSteamTool.dll`; this port
  produces `RustyTools.dll` from the same sources. The library crate
  (`[lib] name = "RustyTools"`) still exports `TokeerUri` via `#[no_mangle]`
  so the `bst://` URI scheme registration works unchanged — the target
  name is a link-time detail, not an ABI detail.

**Verified:**
- `cargo test --lib --workspace` compiles the crate's 352 unit tests
  plus the two new proxy crates with zero new tests — the proxies are
  pure `unsafe` FFI with no logic to unit-test in isolation. Verified by
  inspection against the C++ source.
- `cargo build --workspace` produces three DLLs in `target/debug/`:
  `RustyTools.dll`, `dwmapi.dll`, `xinput1_4.dll`.
- The `dwmapi.def` export table matches the C++ pragma block entry for
  entry (106 total). The `xinput1_4.def` table is byte-identical to the
  C++ `.def`, with the LIBRARY directive kept.

**Tests:** 0 new. Running total: 352.

**Not verified in unit tests (by design):**
- Actual loader hijacking. Testing this requires placing the DLLs next to
  a real `steam.exe` and restarting Steam, which the manual checklist in
  `docs/TESTING.md` (still to be written) will cover.
- The `DwmTether*` and `Dwmp*` forwarders. They are rarely used but were
  present in the C++ export block, so they are preserved.

**Remaining batches:**
- Batch 09 - `dllmain`'s init sequence was already landed in 6g; the CLI
  `main.rs` is still a stub and gets its real shape alongside a
  `docs/TESTING.md` and final README pass in the integration batch.
- `bridge::appticket_write_ownership_ticket` /
  `bridge::appticket_write_encrypted_ticket` remain stubbed. Two-line
  delegations to `utils::tickets::app_ticket::write_*`; deferred as
  called out in 6c and 6g.

### Batch 06g - dllmain init thread + file watchers

**Source files ported:** `dllmain.cpp`, `Utils/Config/LuaFileWatcher.cpp`,
`Utils/Config/ConfigFileWatcher.cpp`. Final batch of the port.

**Added:**
- `src/utils/config/lua_file_watcher.rs` - watches the merged Lua
  directories with one `DirectoryWatch::Watch` per directory. On any
  change: drain, debounce (200 ms), drain again, then
  `lua_config::reload_directories(&dirs, true)`. `start` / `stop` are
  process-global; `stop` joins the worker.
- `src/utils/config/config_file_watcher.rs` - watches the TOML
  config's parent directory and the Lua directory. Events are tagged
  by which watch fired so a config change reloads the TOML (and, if
  `[lua].paths` changed, re-merges the Lua watch set), while a `.lua`
  file event rescans the Lua directories.

**Changed:**
- `src/dllmain.rs` - full rewrite. On top of the module globals
  introduced in 6a:
  * `RuntimePaths` (install / client / ui / diversion / lua_dir /
    config) in a `OnceLock`, populated by `initialize_steam_components`.
  * `InitThread` runs the C++ init sequence in order: logging, module
    load, config, pattern load for both DLLs, IPC metadata, Lua
    parse + watcher, hook install for both modules, missing-pattern
    popup, CloudRedirect init, donor start, URI scheme registration.
  * `is_steam_host` checks the running executable's name; a
    `rundll32 bst://` handler is not `steam.exe`, so the injection
    machinery never runs there.
  * `#[no_mangle] pub unsafe extern "system" fn DllMain` dispatches
    `DLL_PROCESS_ATTACH` (thread spawn, no work under loader lock) and
    `DLL_PROCESS_DETACH` (unhook + shutdown, wrapped in
    `catch_unwind`). Non-Windows targets get a stub export so the
    crate still builds.
- `src/utils/config/mod.rs` - declares the two new watcher modules.

**Verified:**
- `dllmain`: `set_client_hmodule` / `client_hmodule` round-trip;
  `K_ONLINE_FIX_APPID == 480`; `paths()` returns `None` before
  `initialize_steam_components` runs.
- `lua_file_watcher::stop` and `config_file_watcher::stop` are safe to
  call before `start`.

**Tests:** 5 new. Running total: 354.

**The port is complete through all of Hook, Pipe, dllmain, and the
two file watchers.** The remaining entries in `STUBS.md` are
explicitly deferred / non-critical-path, not blockers:

**Deliberately not ported (see STUBS.md):**
- `Utils/Update/AppUpdater.cpp` — self-updater, `#ifdef
  OST_ENABLE_UPDATER` in the C++ build and off in every shipped
  configuration.
- `dwmapi/`, `xinput1_4/` — proxy DLLs, part of the injection
  mechanism rather than the ported library.

**Still stubbed (small, follow-up):**
- `bridge::appticket_write_ownership_ticket` and
  `bridge::appticket_write_encrypted_ticket` still return `false`.
  They delegate to `utils::tickets::app_ticket::write_*`, which is
  real since batch 04. The two-line delegation is deferred rather
  than rolled into 6g to keep this batch focused; it is a
  mechanical change and no caller is blocked on it today.

**Known behavioural divergences:** no new entries.

**Cumulative totals through 6g:** 354 tests, 0 warnings, 0
failures. Every file under `Hook/`, `Pipe/`, `Utils/`, `Steam/`,
`OSTPlatform/`, and `dllmain.cpp` is ported or explicitly documented
as deferred.

### Batch 06f.2 - fix decimal literal in test

**Fixed:**
- `src/pipe/process_inspector.rs` - the test
  `app_id_from_game_id_masks_24_bits` used the decimal literal
  `1224980519775477826`, intending it to equal the hex value
  `0x1100001000000042`. It does not - the two differ in the low 24
  bits, and the hand-typed decimal's low bits are `0xFD9A42`, not
  `0x42`. The function is correct; the input value was wrong. The
  test now constructs the input from the hex literal
  `0x1100_0010_0000_0042` directly, so the equivalence between the
  two representations is not assumed anywhere.

**Note:** the mask contract (`parsed & 0x00FF_FFFF`) is verified by
the passing assertions for the literal and the two zero/non-numeric
cases. No behaviour change.

### Batch 06f.1 - float format specifiers

**Fixed:**
- `src/pipe/process_inspector.rs` and `src/pipe/protection_scan.rs` -
  used the C++ `%f` formatting traits `{:.3f}` and `{:.2f}`, which
  Rust's `format!` does not have. The equivalent in Rust is `{:.3}`
  and `{:.2}` â€” for floating point types, the default `Display` trait
  with an explicit precision prints that many digits after the decimal
  point, matching `%.3f` / `%.2f` in C. Five call sites across two
  files.

**Note for future logging:** Rust's formatting trait vocabulary is
``Display``, `:?` Debug, `:e` / `:E` exponential, `:o`, `:p`, `:b`,
`:x`, `:X`. There is no `:f`. Use `{:.N}` for N decimal places.

### Batch 06f - Pipe layer

**Source files ported:** `Pipe/PipeManager.h`, `Pipe/PipeManager.cpp`,
`Pipe/PipeTypes.h`, `Pipe/ProcessInspector.h`,
`Pipe/ProcessInspector.cpp`,
`Pipe/Features/DenuvoAuth/DenuvoAuth.h`,
`Pipe/Features/DenuvoAuth/DenuvoAuth.cpp`,
`Pipe/Features/DenuvoAuth/ProtectionScan.h`,
`Pipe/Features/DenuvoAuth/ProtectionScan.cpp`,
`Pipe/Features/Injection/Injection.h`,
`Pipe/Features/Injection/Injection.cpp`.

**Added:**
- `src/pipe/pipe_types.rs` - `ProcessKey`, `PipeKey`, `PipeContext`,
  `make_pipe_key`.
- `src/pipe/process_inspector.rs` - `ProcessEnvironment`,
  `ProcessSnapshot`, `inspect_process`, `read_steam_environment`,
  the appid string parsers.
- `src/pipe/protection_scan.rs` - full Denuvo detector: legacy
  section + "DENUVO" string, OEP pattern, and the RWX-blob structural
  fallback with entropy floor. `SectionEntropy` is the Shannon
  entropy of a byte histogram.
- `src/pipe/denuvo_auth.rs` - per-process Denuvo state machine. On
  handshake, ensures `ScanProtection` has run for this process
  (unless `forcedenuvo` in Lua), selects the first pipe as the
  authorization pipe, and advances through `Authorizing` ->
  `EndAuthorization` after the configured handshake count. On end,
  persists the active Steam user's SteamID if the app is owned.
- `src/pipe/injection.rs` - `[[inject]]` entries from the config,
  keyed on `(process, path)` so each DLL injects at most once per
  process.
- `src/pipe/pipe_manager.rs` - `OnHandshake` entry point, the
  appid retry loop, the process-snapshot cache, and the dispatch to
  `DenuvoAuth::apply` / `Injection::apply`.

**Changed:**
- `src/pipe/mod.rs` - declares all six modules.
- `src/bridge.rs` - `pipe_manager_on_handshake` and
  `denuvo_auth_is_authorized_pipe` are now real delegations. The
  `Hooks_IPC::handle_handshake` path and the Denuvo
  authorization-window branch in
  `Hooks_IPC_ISteamUser::handler_post_get_app_ownership_ticket_extended_data`
  become active.
- `src/utils/config/config.rs` - added `get_inject_dlls()` accessor.
  The C++ reads `Config::injectDlls` directly; the Rust port stores
  it inside the mutex'd snapshot, so the accessor clones it out.

**Verified:**
- `ProcessKey::is_valid` and `PipeKey::is_valid` require both fields.
- `make_pipe_key(null)` is invalid.
- `app_id_from_game_id_string` masks 24 bits; rejects zero; rejects
  non-numeric.
- `app_id_from_appid_string` rejects zero; rejects non-numeric.
- `is_steam_process_name` is case-insensitive.
- `ProcessEnvironment::resolve_app_id` prefers overlay, then game,
  then app.
- `SectionEntropy` of empty input is 0; of uniform input is 0; of
  all 256 distinct bytes is 8.
- `ends_with_insensitive` folds case; rejects too-short value.
- `normalize_directory_prefix` appends a backslash and lowercases;
  handles forward slashes.
- `path_starts_with_insensitive` folds case and normalizes
  separators.
- `parse_universe` covers all four known names, case-insensitive.
- `Stage` string mapping.
- `ProcessAuth::new` starts clean.
- `Injection::matches` requires `tracked_app` unless `all_games`;
  requires appid when `when_appids` is non-empty; requires the
  substring in cmdline when `when_cmdline` is non-empty.
- `PipeManager::on_handshake(null)` is a no-op.
- `scan_protection(0)` returns an empty report.

**Tests:** 24 new (pipe_types 3, process_inspector 5, protection_scan 9,
denuvo 3, injection 3, pipe_manager 2, minus overlap). Running total:
~348.

**Design notes:**
- The C++ has three plain `std::unordered_map` caches with the comment
  "single-threaded, no lock needed". Rust cannot prove that, so each
  is behind a `parking_lot::Mutex`. Behaviour is identical; the
  compiler just cannot see the C++ developer's reasoning.
- `denuvo_auth::is_authorized_pipe` reads the process auth and then
  clones it out of the lock before querying, to avoid holding the
  `G_PROCESS_AUTH` lock while `G_PIPE_PROCESS` is being read (the two
  locks are never taken in a conflicting order, but the guard types
  would otherwise interact awkwardly). The clone is a handful of
  scalars.
- `Injection::matches` uses `String::contains`, the Rust analogue of
  the C++ `std::string::find != npos`. Same substring semantics.

**Still stubbed (pending 6g):**
- `dllmain`'s init thread — the module globals landed in 6a; the
  worker thread that loads `steamclient64.dll`, resolves patterns,
  parses Lua, installs hooks, starts the watchers and the donor,
  registers the URI scheme, and calls `DetourRestoreAfterWith` is
  the last piece.
- `bridge::appticket_write_ownership_ticket` and
  `appticket_write_encrypted_ticket` — two-line delegations to
  `utils::tickets::app_ticket::write_*`. Deferred to a follow-up.
- `Utils/Config/LuaFileWatcher.cpp` and
  `Utils/Config/ConfigFileWatcher.cpp` — file watchers, part of 6g.
- `Utils/Update/` — deferred (self-updater, out of scope).

**Known behavioural divergences:** no new entries.

### Batch 06e-3b.1 - visibility and missing eMsg constant

**Fixed:**
- `netpacket/packet.rs` - added `K_EMSG_CLIENT_GET_LEGACY_GAME_KEY = 730`
  next to the other emitted-eMsg constants. The value was previously
  only defined inside `legacy_key.rs`, where it was private. The
  `BBuildAndAsyncSendFrame` hook needs it and cannot import a private
  constant from a sibling module.
- `netpacket/manifest_probe.rs` - `K_PENDING_TIMEOUT` made `pub`.
  `netpacket/mod.rs` uses it as the timeout for the blocking
  `request_manifest_code` and the detached `probe_manifest` thread.
- `netpacket/legacy_key.rs` - the local 730 constant now sources its
  value from `packet.rs`, so there is a single definition of that
  eMsg value.

**Note:** the 6e-3b patch emitted a spurious
`'t' is not recognized as an internal or external command` before its
own output. That came from a stray byte in an earlier command in the
same console session, not from the patch script; the script itself ran
to completion.

### Batch 06e-3b - Hooks: Manifest + Cloud + LegacyKey + bridges

**Source files ported:** the remaining `Hooks_NetPacket.cpp`
sub-namespaces (`Manifest`, `Cloud`, `LegacyKey`) and the final
`RecvPkt` / `BBuildAndAsyncSendFrame` wiring.

**Added:**
- `src/hook/netpacket/manifest.rs` - `Hooks_NetPacket_Manifest`. On
  send: records the `(depot, gid)` correlation for passive capture,
  pre-seeds the cache for the exact gid Steam is asking about (gated
  on `ActiveDownloadCount() > 0`, so scheduled retries stay silent),
  and launches a background HTTP fetch over a channel. On recv: waits
  up to 12 s for the fetch, patches the header (`eresult = OK`) and
  body (`manifest_request_code`) when the fetch succeeds, and submits
  the genuine code Steam returned to `ManifestDonor` for passive
  capture.
- `src/hook/netpacket/cloud.rs` - `Hooks_NetPacket_Cloud`.
  `handle_send` reads the appid from a `Cloud.*` request body via a
  minimal varint walker (no proto definitions needed), calls
  `CloudRedirectHost::HandleCloudRpc`, and builds a synthesized eMsg
  147 response packet into a bounded queue. `drain` delivers queued
  responses by borrowing the carrier packet for one call to the
  original, restoring the pointer afterward.
- `src/hook/netpacket/legacy_key.rs` - `Hooks_NetPacket_LegacyKey`.
  Intercepts the non-proto eMsg 730 (struct message that `unpack_raw`
  cannot parse), resolves a legacy CD key via `LegacyCDKey::Resolve`,
  and queues a synthesized eMsg 785 response. `drain` delivers it the
  same way as Cloud.

**Changed:**
- `src/hook/netpacket/mod.rs`:
  * `BBuildAndAsyncSendFrame` now checks for eMsg 730 before
    `unpack_raw`. If `LegacyKey::HandleSend` answers locally, the
    frame is suppressed (returns `true` so Steam thinks it was sent).
  * `RecvPkt` now drains the Cloud and LegacyKey queues after
    RichPresence's inject, matching the C++ call order.
  * `request_manifest_code` becomes real: delegates to
    `manifest_probe::request`, blocking with a timeout.
  * `probe_manifest` becomes real: resolves the depot via
    `hook::manifest::lookup_depot`, then spawns a detached thread for
    the request and returns.
- `src/hook/netpacket/dispatch.rs`:
  * The `Cloud.*` branch in `send_service_job` is now live. `SignalApp
    ExitSyncDone#1` and `ClientConflictResolution#1` still pass
    through untouched. Gated on `netpkt::is_resolved()`, since the
    drain is inert until the packet layout is known.
  * The `HASH_JOB_GET_MANIFEST_REQUEST_CODE` case on send now calls
    `manifest::handle_send` and returns `None` (the outgoing frame is
    never modified). On recv, it tries `manifest_probe::handle_recv`
    first and falls through to `manifest::handle_recv` when the probe
    did not own the reply.
- `src/bridge.rs` - `hooks_probe_manifest` and
  `hooks_request_manifest_code` now delegate to the real netpacket
  API. All hook-layer bridge rows in `STUBS.md` are resolved.

**Verified:**
- `manifest::handle_send` on malformed input does not panic.
- `manifest::handle_recv` returns `None` on a malformed header, on a
  header without `jobid_target`, and on a `jobid_target` that was not
  registered by a matching send.
- `cloud::read_varint` reads single-byte and multi-byte varints.
- `cloud::find_varint_field` locates field 1 and `extract_app_id`
  switches to field 2 for `Cloud.ClientCommitFileUpload#1`.
- `legacy_key::handle_send` rejects frames shorter than
  `ExtendedMsgHdr + 4` and frames whose appid is not in the (empty)
  Lua config.
- `legacy_key::has_pending` and `cloud::has_pending` are callable on
  an empty queue.

**Tests:** 11 new (manifest 5, cloud 5, legacy 3, minus adjustments).
Running total: ~322.

**Design notes:**
- Cloud's `handle_send` builds a full `MsgHdr + proto header + body`
  packet and queues it. The drain swaps the carrier packet's data
  pointer to that buffer for one call to the original, exactly like
  RichPresence's inject. This is the C++ "Approach D".
- Manifest's `handle_send` and `handle_recv` use an
  `mpsc::channel` in place of the C++ `std::shared_future`. Single-
  producer / single-consumer is sufficient: the detached fetch thread
  sends once, the recv handler receives once.
- LegacyKey's request-header field offsets are hard-coded byte
  offsets into the packed `ExtendedMsgHdr` rather than accesses via a
  `#[repr(C, packed)]` reference, because the packed struct's
  unaligned fields cannot be borrowed. The offsets (7, 15, 24) match
  the struct layout and are asserted in a test.

**Netpacket layer complete.** Every handler in the C++
`Hooks_NetPacket.cpp` is now ported or explicitly preserved as a
commented-out case (the eTicket eMsg 5527 path, matching the C++
"migrated to IPC Layer" comment). `bridge::hooks_*` are all real.

**Still stubbed (6f and 6g):**
- `Pipe/` layer — `bridge::pipe_manager_on_handshake` and
  `bridge::denuvo_auth_is_authorized_pipe`.
- `dllmain`'s init thread — deferred to 6g.
- `bridge::appticket_write_ownership_ticket` and
  `appticket_write_encrypted_ticket` — two-line delegations, will land
  in a follow-up.
- `Utils/Update/` — deferred (self-updater, out of scope).

**Known behavioural divergences:** no new entries.

### Batch 06e-3a - Hooks: ETicket + OwnershipTicket + ManifestProbe

**Source files ported (partial):** `Hook/Hooks_NetPacket.cpp` — the
`Hooks_NetPacket_ETicket`, `Hooks_NetPacket_OwnershipTicket`, and
`Hooks_NetPacket_ManifestProbe` sub-namespaces.

**Added:**
- `src/hook/netpacket/eticket.rs` - `Hooks_NetPacket_ETicket`. Ports
  the `CMsgClientRequestEncryptedAppTicketResponse` handler. **Not
  wired:** the C++ dispatch case is commented out with the note
  "migrated to IPC Layer
  `Hooks_IPC_ISteamUser::GetEncryptedAppTicketResponse`". The handler
  is preserved here so a future patch can re-enable it by uncommenting
  one line in `dispatch.rs`.
- `src/hook/netpacket/ownershipticket.rs` -
  `Hooks_NetPacket_OwnershipTicket`. Incoming eMsg 858. On a non-OK
  response for an app in the Lua config, serves a credential-store
  ticket when one exists, otherwise mints one from the backend, and
  sets `eresult = OK`. When the credential store already holds a
  valid ticket, its bytes are preferred so the 858 response and the
  IPC `GetAppOwnershipTicketExtendedData` path hand Denuvo identical
  bytes.
- `src/hook/netpacket/manifest_probe.rs` -
  `Hooks_NetPacket_ManifestProbe`. Originates
  `ContentServerDirectory.GetManifestRequestCode#1`. Captures the
  websocket object and send trampoline from the binary-frame send
  hook, keeps one real 151 header as a template, and correlates the
  reply via a high-range `jobid_source`.

**Changed:**
- `src/hook/netpacket/mod.rs`:
  * `BBuildAndAsyncSendFrame` calls `manifest_probe::capture_context`
    on every binary frame, capturing the connection object and the
    trampoline address. A reconnect updates both.
  * Declares `pub mod eticket; pub mod ownershipticket; pub mod
    manifest_probe;`.
  * Public API stubs `request_manifest_code` and `probe_manifest`
    remain stubbed until 6e-3b.
- `src/hook/netpacket/dispatch.rs`:
  * The 151 case in `send_frame` calls
    `manifest_probe::capture_context` with the frame's header, so the
    template is captured.
  * The 858 case in `recv_frame` now calls
    `ownershipticket::handle_recv`.
  * The `GetManifestRequestCode` service-job case in `recv_service_job`
    calls `manifest_probe::handle_recv` before falling through. The
    Manifest handler (6e-3b) is not yet wired, so this is a no-op on
    the body but does resolve the pending probe entry.

**Design notes:**
- The C++ returns a `std::future<uint64_t>` from `Request` so a
  caller can spawn a thread to wait. Rust has no standard future
  whose result can be waited on with a timeout from synchronous code,
  so the port uses `std::sync::mpsc::channel` and returns the
  `Receiver<u64>`. Semantics match: a value of `0` means "failed,
  refused, or timed out". Callers that do not need a timeout can just
  `rx.recv()`.
- The captured `p_websocket` and `send_frame` are stored as `usize`
  rather than raw pointers and fn pointers respectively. That avoids
  a blanket `unsafe impl Send for ProbeState` and matches the
  approach used for `trap::HandlerHandle` in `veh_common.rs`.
- The header template is only captured when it is currently empty, so
  the first real outbound 151 header wins. The C++ does the same
  (`if (g_HdrTemplate.empty() && pHdr && cbHdr)`).

**Verified:**
- `eticket::handle_recv` returns `None` on malformed input, on an OK
  response, and on a response whose `app_id` is absent.
- `ownershipticket::handle_recv` returns `None` on malformed input,
  on an OK response, and on a non-OK response whose `app_id` is not
  in the (empty) Lua config.
- `manifest_probe::handle_recv` returns `false` on a malformed
  header, a header without `jobid_target`, and a header whose
  `jobid_target` is below the reserved range.
- `K_JOB_ID_BASE > 0x7E00_0000_0000_0000` (sanity).

**Tests:** 9 new (eticket 2, ownershipticket 3, manifest_probe 4).
Running total: 314.

**Still stubbed (pending 6e-3b):**
- `Hooks_NetPacket_Manifest` (send handler, recv-with-timeout, cache
  pre-seed).
- `Hooks_NetPacket_Cloud` (CloudRedirect integration).
- `Hooks_NetPacket_LegacyKey` (non-proto struct message interception).
- `netpacket::request_manifest_code` and `netpacket::probe_manifest`
  go real (currently return 0 / false).
- `bridge::hooks_probe_manifest` and
  `bridge::hooks_request_manifest_code` become real.
- The `Cloud.` prefix branch in `send_service_job` returns
  `SendReplacement::Suppress` when CloudRedirect answers an RPC.
- The Cloud and LegacyKey `Drain` calls are added to `RecvPkt`.

**Known behavioural divergences:** no new entries.

### Batch 06e-2.1 - fix Replacment rename and Kv path

**Fixed:**
- `netpacket/family_sharing.rs` - still imported `super::dispatch::Replacement`,
  which 6e-2 renamed to the two split enums. `clear_body` now returns a
  `RecvReplacement` and the import reflects that.
- `netpacket/richpresence.rs` - `Kv` is a nested type inside the
  `Friend` message, not directly inside `CMsgClientPersonaState`.
  The path is
  `crate::steam::proto::c_msg_client_persona_state::friend::Kv`.

### Batch 06e-2 - Hooks: UserStats + RichPresence

**Source files ported (partial):** `Hook/Hooks_NetPacket.cpp` — the
`Hooks_NetPacket_UserStats` and `Hooks_NetPacket_RichPresence`
sub-namespaces.

**Added:**
- `src/hook/netpacket/userstats.rs` - four handlers:
  * `handle_send_get_user_stats` — outgoing eMsg 151 with target
    `Player.GetUserStats#1`. Stores `jobid_source -> appid`, patches
    `steamid` to `LuaConfig::GetStatSteamId`.
  * `handle_recv_get_user_stats_response` — incoming eMsg 147 with
    the same target. Sets `eresult = OK` on the header; if the
    `jobid_target` matches a stored request and the appid is in our
    config, strips `stats` from the body.
  * `handle_send_client_get_user_stats` — outgoing eMsg 818. Forces
    `schema_local_version = -1` and sets `steam_id_for_user`.
  * `handle_recv_client_get_user_stats_response` — incoming eMsg 819.
    Clears `stats` / `achievement_blocks`, sets `eresult = OK`, and
    overlays CloudRedirect's cloud-synced achievements (with a
    running `crc_stats`).
- `src/hook/netpacket/richpresence.rs` - full port, replacing the
  6e-1 stub:
  * KV1 binary walker (`extract_string_kvs`) for
    `CMsgClientRichPresenceUpload.rich_presence_kv`.
  * `track_send_games_played` — captures `SteamID` from the first
    outbound header, tracks the topmost `games_played` entry, and
    stages an inject packet on change.
  * `track_rp_send` — refreshes the KV cache for the current appid
    and re-stages the inject.
  * `handle_recv` — caches the raw `CMsgClientPersonaState` bytes as
    the inject template; if tracking, patches our own entry in place
    and returns the new body.
  * `try_inject` — delivers the staged inject via the carrier packet
    in `RecvPkt`. The carrier's data pointer is swapped for one call
    to the original and then restored.

**Changed:**
- `src/hook/netpacket/dispatch.rs` — the single `Replacement` enum from
  6e-1 is split into `SendReplacement` (`None` / `Body` / `Suppress`)
  and `RecvReplacement` (`None` / `Body` / `Hdr` / `Both`). The C++
  had two distinct outcome shapes on the two sides; forcing them into
  one type required branches that don't exist in the C++. The wire-up
  now covers the new UserStats and RichPresence handlers.
- `src/hook/netpacket/packet.rs` — `replace_recv_packet` now takes
  optional new-header and new-body arguments. Passing `None` keeps the
  corresponding part of the original. Previously it always replaced
  both, which was fine for 6e-1 (all handlers replaced the body only)
  but not for a handler like `handle_recv_get_user_stats_response`
  that changes only the header when no appid matches.
- `src/hook/netpacket/mod.rs`:
  * `BBuildAndAsyncSendFrame` matches on `SendReplacement`.
  * `RecvPkt` calls `richpresence::try_inject` before dispatching the
    carrier packet, matching the C++ order (resolve layout, inject,
    drain, dispatch). The Cloud and LegacyKey drains land in 6e-3.
  * `RecvPkt` matches on `RecvReplacement`.
  * Declares `pub mod userstats;` and re-exports the two enums.
- `src/hook/netpacket/state.rs` — dropped the `last_send_body` field.
  The 6e-1 hook used it as a workaround for having a single outcome
  enum; with `SendReplacement::Body` carrying the body directly, it is
  not needed.

**Verified:**
- `handle_send_get_user_stats` / `handle_send_client_get_user_stats`
  return `None` on malformed input.
- `handle_recv_client_get_user_stats_response` returns `None` on
  malformed input.
- `handle_recv_get_user_stats_response` returns `None` on a malformed
  header.
- `extract_string_kvs` on an empty input leaves the output empty; on
  a flat struct (`0x00 name 0x01 key val 0x08`) extracts one pair; on
  an unrecognised type byte terminates the walk with no output.
- `playing_app_id`, `local_steam_id`, and `is_inject_pending` default
  to zero/false.
- Send and receive replacement enums each have a `None` default.
- The three service-job hashes remain distinct.

**Tests:** 12 new (userstats 4, richpresence 4, dispatch 4, minus
adjustments). Running total: 305.

**Design notes / divergences:**
- The C++ `Hooks_NetPacket_OnlineFix::HandleSend` and
  `Hooks_NetPacket_RichPresence::TrackSend` share a single parsed
  `CMsgClientGamesPlayed`. The Rust dispatch parses it twice — once
  in `track_send_games_played` and once in `onlinefix::handle_send`.
  Functionally identical, one extra allocation per outbound
  GamesPlayed frame (a low-frequency message).
- The C++ `HandleRecv_GetUserStatsResponse` writes the re-serialized
  response *in place* into `pBody` and sets `g_ResizedInPlace`. The
  Rust version returns a `RecvReplacement::Both` and lets
  `RecvPkt` put it into the pool, which is the same pattern every
  other body-replacing handler uses. Same observable result, the
  packet's data pointer points into a pool slot rather than the
  original buffer.
- The C++ `HandleRecv` for PersonaState writes into `g_NewBody` and
  returns `true`; the Rust version returns `Some(new_body)`. The
  cache of the *original* bytes as inject template happens before
  the mutation in both.

**Still stubbed (pending 6e-3):**
- `Hooks_NetPacket_ETicket`, `Hooks_NetPacket_OwnershipTicket`,
  `Hooks_NetPacket_Manifest`, `Hooks_NetPacket_ManifestProbe`,
  `Hooks_NetPacket_Cloud`, `Hooks_NetPacket_LegacyKey`.
- `netpacket::request_manifest_code` and `netpacket::probe_manifest`
  go real.
- The `Cloud.` prefix branch in `send_service_job` becomes active and
  returns `SendReplacement::Suppress` when CloudRedirect answers an
  RPC.
- The Cloud and LegacyKey `Drain` calls are added to `RecvPkt`.
- `bridge::hooks_probe_manifest` and
  `bridge::hooks_request_manifest_code` become real.

**Known behavioural divergences:** no new entries.

### Batch 06e-1.5 - function-cast warnings + stack overflow fix

**Fixed:**
- `src/hook/macros.rs` - both `__install_hook_step!` and
  `uninstall_hook!` cast the hook function item directly to `usize` and
  then to `*mut c_void`. Rust's `function_casts_as_integer` lint wants
  a pointer intermediate, so the cast now reads
  `... as *const () as usize as *mut c_void`. Silences 28 warnings
  across every hook module.
- `src/hook/netpacket/state.rs` - the test `new_state_is_zeroed`
  constructed a `NetPacketState` by value. The struct is ~1.18 MiB
  (two 8-entry ring pools of ~65 KiB each, plus header and body
  scratch buffers); in a debug build the test harness's 2 MiB thread
  stack overflowed. The test now reads the process-static `STATE`
  directly. The static lives in `.bss` and has no stack cost.

### Batch 06e-1.4 - add missing module prefix

**Fixed:**
- `netpacket/access_token.rs` - the 6e-1.3 substitution replaced the
  full path `CMsgClientPicsProductInfoRequest::AppInfo` with the bare
  module name `c_msg_client_pics_product_info_request::AppInfo`, losing
  the `crate::steam::proto::` prefix. Re-added.

### Batch 06e-1.3 - fix nested-type module paths

**Fixed:**
- `netpacket/access_token.rs`, `netpacket/licenses.rs`,
  `netpacket/onlinefix.rs` - nested proto types are accessed through
  a lowercase module named after the parent message, not through the
  parent struct itself. The parent struct has no `pub use` re-export
  for its children:
  * `c_msg_client_pics_product_info_request::AppInfo`
    (not `CMsgClientPicsProductInfoRequest::AppInfo` - that is an
    ambiguous associated-type error)
  * `c_msg_client_license_list::License`
  * `c_msg_client_games_played::GamePlayed`

**Note on the module names:** prost applies its own `to_snake_case`
to the original proto identifier, treating a run of capitals as one
word and inserting an underscore at real word boundaries. So
`CMsgClientPICSProductInfoRequest` yields the module
`c_msg_client_pics_product_info_request`, with an underscore between
`pics` and `product`. The 6e-1.2 note said the module name was wrong
when in fact the earlier `c_msg_client_picsproduct_info_request`
(spelled without the underscore) had been wrong; the "wrong-shape"
diagnosis was itself wrong.

### Batch 06e-1.2 - prost API and naming fixes

**Fixed:**
- `netpacket/mod.rs` - added `resolve_ready` to the `use crate::{...}`
  import list. `#[macro_export]` macros are not implicitly in scope in
  nested modules; every consumer so far imports them explicitly.
- `netpacket/packet.rs`, `netpacket/dispatch.rs`,
  `netpacket/access_token.rs`, `netpacket/licenses.rs`,
  `netpacket/onlinefix.rs` - changed every
  `prost::Message::decode::<T>(buf)` to `T::decode(buf)`. prost 0.13's
  `Message::decode` is not a generic method; the type is inferred from
  the receiver. Requires `use prost::Message;` in scope, which is now
  present in each file.
- `netpacket/access_token.rs` - prost renames `CMsgClientPICSProductInfoRequest`
  to `CMsgClientPicsProductInfoRequest`: a run of capitals is treated
  as a single word (`PICS` -> `Pics`). All references updated. The
  nested `AppInfo` type is `CMsgClientPicsProductInfoRequest::AppInfo`,
  not `c_msg_client_picsproduct_info_request::AppInfo`.
- `netpacket/licenses.rs` - nested `License` type is
  `CMsgClientLicenseList::License`, not
  `c_msg_client_license_list::License`.
- `netpacket/onlinefix.rs` - nested `GamePlayed` type is
  `CMsgClientGamesPlayed::GamePlayed`, not
  `c_msg_client_games_played::GamePlayed`.

**Note for future prost work:** prost applies two transformations to
`.proto` identifiers that this crate must account for at every call
site:
  1. Type names: runs of capitals collapse to one word (`PICS` ->
     `Pics`, `UGC` -> `Ugc`).
  2. Nested types are `Parent::Child`, not `parent_module::Child`.
When in doubt, let the compiler guide you - the E0432 suggestion
literally names the correct path.

### Batch 06e-1.1 - remove bad import in netpacket/mod.rs

**Fixed:**
- `src/hook/netpacket/mod.rs` had a malformed import line
  (`use crate::steam::types::EWebSocketOpCode? ;`) that both used the
  wrong module path and carried a stray `?`. The type was not needed:
  the `BBuildAndAsyncSendFrame` hook signature takes `op_code: u32`
  and compares against a local `K_BINARY` constant, matching the C++
  `EWebSocketOpCode` enum values as raw integers. Line removed.

### Batch 06e-1 - Hooks: NetPacket infrastructure + simple handlers

**Source files ported (partial):** `Hook/Hooks_NetPacket.h`,
`Hook/Hooks_NetPacket.cpp` — the shared infrastructure, the two
top-level hooks, and the AccessToken / FamilySharing / Licenses /
OnlineFix sub-namespaces.

**Added:**
- `src/hook/netpacket/mod.rs` - module tree, `msg_name` helper, the
  `BBuildAndAsyncSendFrame` and `RecvPkt` hooks, `install` / `uninstall`.
  Public API `request_manifest_code` and `probe_manifest` are stubs
  until 6e-3.
- `src/hook/netpacket/state.rs` - the process-global packet state (ring
  pools, header/body scratch buffers, layout probe counters) behind a
  `Sync`-wrapped `UnsafeCell`. Written only from the network thread
  inside the two hooks; no locking.
- `src/hook/netpacket/packet.rs` - `unpack_raw`, `probe_layout`,
  `try_resolve_layout`, `replace_recv_packet`, plus the emitted-eMsg
  constants extracted from the C++ `case` labels.
- `src/hook/netpacket/dispatch.rs` - `Replacement` enum, `send_frame`,
  `recv_frame`, `send_service_job`, `recv_service_job`, and the three
  FNV-1a hash constants for the service-job names.
- `src/hook/netpacket/access_token.rs` - `Hooks_NetPacket_AccessToken`.
  Injects `access_token` into each outgoing
  `CMsgClientPICSProductInfoRequest.apps[i]` whose appid is in Lua's
  `addtoken` set.
- `src/hook/netpacket/family_sharing.rs` -
  `Hooks_NetPacket_FamilySharing`. Clears the body of
  `FamilyGroupsClient.NotifyRunningApps#1` and
  `CMsgClientSharedLibraryStopPlaying`.
- `src/hook/netpacket/licenses.rs` - `Hooks_NetPacket_Licenses`. Parses
  `CMsgClientLicenseList` and forwards the list to
  `hook::package::on_license_list`.
- `src/hook/netpacket/onlinefix.rs` - `Hooks_NetPacket_OnlineFix`.
  Replaces `game_extra_info` with the real game name when a
  `-onlinefix` launch reports appid 480.
- `src/hook/netpacket/richpresence.rs` - stub. The full port lands in
  6e-2 with `UserStats`.

**Added (macros):**
- `resolve_ready!` appended to `src/hook/macros.rs`. Companion to
  `capture_ready!` for `resolve_fn!`-only slots (no captured `this`).

**Changed:**
- `src/hook/mod.rs` - declares `netpacket`.
- `src/hook/framework.rs` - `install_steamclient_hooks` /
  `uninstall_steamclient_hooks` now include `netpacket::*`, positioned
  between `misc` and `package` to match `HookManager.cpp`.

**Design notes:**
- The C++ communicates handler outcomes by setting process-global
  `g_NeedReplaceBody` / `g_cbNewBody` / `g_NeedReplaceHdr` flags the
  hook reads afterward. In Rust dispatch returns a `Replacement` enum
  and the hook matches on it. Behaviour is identical; the compiler
  enforces that every outcome is handled.
- The C++ `g_ResizedInPlace` flag (used by `HandleRecv_GetUserStatsResponse`
  to shrink a body in place without repointing the packet) is not
  modelled yet — that handler lands in 6e-2. When it does, `Replacement`
  gains a `BodyShrunk` variant.
- `replace_recv_packet` writes into the ring pool and points the packet
  at the pool buffer permanently, matching the C++ `ReplaceRecvPacket`.
  The pool is not restored after the original `RecvPkt` returns.
- The `Replacement::Suppress` and `Replacement::Both` variants are
  defined now but not produced by any 6e-1 handler. They exist so 6e-2
  and 6e-3 can add producers without changing the enum.

**Verified:**
- `unpack_raw` rejects short frames, frames without the proto flag, and
  frames with a `headerLength` that runs past the frame; splits header
  and body at the correct offsets; accepts an empty body.
- `msg_name` returns `"?"` before the resolver has been installed.
- `replacement_none_is_default` and `recv_service_job_unknown_target_is_none`
  exercise the dispatch fall-through.
- The three service-job hashes are distinct.
- `family_sharing::clear_body` returns an empty `Body`.
- `access_token::handle_send` returns `None` on a malformed body and on
  a well-formed request whose appids are not in Lua's addappid set.
- `licenses::handle_recv` does not panic on a malformed body, on an
  empty list, or on a one-entry list.
- `onlinefix::handle_send` returns `None` for a game id that is not 480
  and for a malformed body.

**Tests:** 14 new. Running total: 289.

**Still stubbed (pending 6e-2 / 6e-3):**
- 6e-2: `Hooks_NetPacket_UserStats` (four handlers),
  `Hooks_NetPacket_RichPresence` (currently stub).
- 6e-3: `Hooks_NetPacket_ETicket`,
  `Hooks_NetPacket_OwnershipTicket`, `Hooks_NetPacket_Manifest`,
  `Hooks_NetPacket_ManifestProbe`, `Hooks_NetPacket_Cloud`,
  `Hooks_NetPacket_LegacyKey`. `netpacket::request_manifest_code` and
  `netpacket::probe_manifest` go real.
- `bridge::hooks_probe_manifest` and `bridge::hooks_request_manifest_code`
  become real when 6e-3 lands. Until then they remain the current stubs.

**Known behavioural divergences:** no new entries.

### Batch 06d.1 - fix CSteamID imports

**Fixed:**
- `src/steam/ipc_facades.rs` and `src/hook/ipc_isteamuser.rs` imported
  `CSteamID` from `steam::types`, where it does not exist. It lives in
  `steam::structs`. Both imports split into two lines, one for the
  correct module and one for `SteamAPICall_t` / `AppId_t` as before.

### Batch 06d - Hooks: IPC layer

**Source files ported:** `Hook/Hooks_IPC.h`, `Hook/Hooks_IPC.cpp`,
`Hook/Hooks_IPC_ISteamUser.h`, `Hook/Hooks_IPC_ISteamUser.cpp`,
`Hook/Hooks_IPC_ISteamUtils.h`, `Hook/Hooks_IPC_ISteamUtils.cpp`,
and the `IPCMessages.gen.h` facades from `Steam/IPCMessages.steamd`.

**Added:**
- `src/steam/ipc_facades.rs` - hand-maintained port of the C++
  generated `IPCMessages.gen.h`. Provides `BufView` (a view over a
  `CUtlBuffer`'s valid prefix), the three envelope types
  (`IPCRequest`, `IPCInterfaceCall`, `IPCResponse`) plus
  `IPCHandshakeReq`, and one request/response facade per method that
  the `.steamd` declares. The `.steamd` remains the source of truth;
  this file must be kept in lockstep.
- `src/hook/ipc.rs` - `Hooks_IPC`. `IPCProcessMessage` hook, the
  `(interfaceID, funcHash)` dispatch table, handshake handling, and
  NetworkingSockets detection. `install` resolves `GetPipeClient` and
  calls `register` on the two per-interface modules before opening the
  detour transaction.
- `src/hook/ipc_isteamuser.rs` - the four `IClientUser` post-handlers:
  `GetSteamID`, `GetAppOwnershipTicketExtendedData`,
  `RequestEncryptedAppTicket`, `GetEncryptedAppTicket`.
- `src/hook/ipc_isteamutils.rs` - the two `IClientUtils` post-handlers
  (`GetAppID`, `GetAPICallResult`) plus the `write_api_call_result`
  helper.

**Changed:**
- `src/steam/mod.rs` - declares `pub mod ipc_facades;`.
- `src/hook/mod.rs` - declares `ipc`, `ipc_isteamuser`, `ipc_isteamutils`.
- `src/hook/framework.rs` - `install_steamclient_hooks` /
  `uninstall_steamclient_hooks` now include `ipc::*`, positioned
  between `decryption` and `manifest` to match `HookManager.cpp`.
- `src/bridge.rs` - added two 6f-bound stubs:
  `pipe_manager_on_handshake` (no-op) and
  `denuvo_auth_is_authorized_pipe` (returns `false`).
  `pipe_manager_on_handshake` is called from `handle_handshake` after
  the PID is set; `denuvo_auth_is_authorized_pipe` is called from the
  GetAppOwnershipTicketExtendedData handler, so outside 6f every
  request falls back to `CredentialStoreThenForge`.

**Design notes:**
- The C++ `IPCHandlerFn` is a raw fn pointer that can be null; the Rust
  `HandlerEntry` holds `Option<IpcHandlerFn>`. `ResolvedHandler` is
  `Copy`, so `find_handler` returns a copied value and does not hold
  the mutex across the detour call.
- `register_handlers` looks up each entry via
  `ipc_loader::find_by_name`. If the IPCLoader metadata is unavailable
  (as in unit tests, where the loader has not run), no handlers
  register and the log line notes the missing spec — matching the C++
  `[Handler Disabled]` path.
- The `write_api_call_result` template constrains `T` to `Copy`, the
  Rust analogue of the C++ `std::is_trivially_copyable` static assert.

**Verified:**
- `BufView` bounds checks on out-of-range reads/writes and slices.
  `copy_bytes` truncates short input and zero-fills the remainder,
  matching `detail::CopyBytes`.
- `IPCRequest::ok` / `command_raw` on empty, short, and valid buffers.
- `IPCInterfaceCall::body` slicing with a 21-byte body: header at
  bytes 0-8, payload at 9-16, fencepost at 17-20.
- `IPCHandshakeReq` reads version/pid/tid at the correct offsets.
- `GetAppIDResp` round-trips a u32 return value at body offset 0.
- `GetSteamIDResp` round-trips a `CSteamID` through a u64.
- `GetAPICallResultResp` writes the callback payload at the correct
  offset with zero-padding, and pbFailed at `1 + cub_callback`.
- `GetAppOwnershipTicketExtendedDataResp` layout: returnValue at body
  offset 0, pTicket at 4, four u32s after that, all at
  `4/8/12/16 + cb_max_ticket`.
- `find_handler` returns `None` on an empty registry.

**Tests:** 14 new (`ipc_facades` 12, `ipc` 2). Running total: 274.

**Still stubbed (pending 6e-6f):**
- `Hooks_NetPacket` (6e) - includes `bridge::hooks_probe_manifest` and
  `bridge::hooks_request_manifest_code`.
- `Pipe/` (6f) - `bridge::pipe_manager_on_handshake` and
  `bridge::denuvo_auth_is_authorized_pipe` go real; `Hooks_IPC`'s
  handshake path and the Denuvo auth-window branch become active.
- `bridge::appticket_write_ownership_ticket` /
  `appticket_write_encrypted_ticket` still return `false`; two-line
  delegation to `utils::tickets::app_ticket::write_*` deferred to a
  follow-up patch.
- `dllmain`'s init thread - deferred to 6g.

**Known behavioural divergences:** no new entries.

### Batch 06c.1 - rename prost output to steam_messages.rs

**Fixed:**
- `build.rs` - `prost-build` names its output after the proto's
  `package` declaration. `proto/steam_messages.proto` has no `package`
  line, so the generated file landed in `OUT_DIR` as `_.rs` and the
  `include!(concat!(env!("OUT_DIR"), "/steam_messages.rs"))` in
  `src/steam/proto.rs` failed with "could not read ... steam_messages.rs".
  The build script now renames `_.rs` to `steam_messages.rs` after
  `compile_protos` runs.

**Design note:** renaming in `build.rs` (rather than adding a `package`
line to the proto) was chosen so the proto stays untouched and the
generated types remain at the module root. Adding a `package` would
require `crate::steam::proto::<package>::CMsgProtoBufHeader` at every
call site. The rename is idempotent: a stale target from a previous run
is removed before the rename, and the build fails loudly if prost
produced neither name.

### Batch 06c - Hooks: Package + Manifest

**Source files ported:** `Hook/Hooks_Package.h`, `Hook/Hooks_Package.cpp`,
`Hook/Hooks_Manifest.h`, `Hook/Hooks_Manifest.cpp`.

**Added:**
- `src/steam/proto.rs` - `include!`s the `prost-build` output from
  `OUT_DIR`. Field numbers and wire types are preserved exactly from
  `proto/steam_messages.proto`. The prost runtime uses a different
  in-memory representation from the C++ protobuf runtime, so code paths
  that need to pass a message through a C++ protobuf helper cannot be
  ported directly — see the `Hooks_SteamUI::BuildCompleteAppOverviewChange`
  divergence documented in `STUBS.md`.
- `src/hook/package.rs` - `Hooks_Package`. Resolves `CUtlMemoryGrow` /
  `MarkLicenseAsChanged` / `ProcessPendingLicenseUpdates`, captures
  `GetPackageInfo` via int3, and hooks `CheckAppOwnership` to fake
  ownership of Lua-declared apps. Exposes `on_license_list`,
  `try_dump_owned_depots`, `get_owned_depots`, and
  `notify_license_changed`.
- `src/hook/manifest.rs` - `Hooks_Manifest`. Hooks
  `BuildDepotDependency` (records depot<->app<->gid, applies
  `setManifestid` overrides, guards against the appinfo-mid-refresh
  transient) and `YldLoadDepotManifest` (pre-seeds the manifest cache
  on per-manifest acquire). Exposes `lookup_depot` and
  `report_missing_manifest`.

**Changed:**
- `build.rs` - now runs `prost-build` against
  `proto/steam_messages.proto` on every build. Uses the vendored
  `protoc` from `protoc-bin-vendored` when available. Fails loudly if
  the proto file is missing.
- `src/steam/mod.rs` - declares `pub mod proto;`.
- `src/hook/mod.rs` - declares `package` and `manifest`.
- `src/hook/framework.rs` - `install_steamclient_hooks` /
  `uninstall_steamclient_hooks` now call `manifest::*` and
  `package::*` in the same order as `HookManager.cpp`.
- `src/bridge.rs` - `hooks_notify_license_changed`,
  `hooks_get_owned_depots`, and `hooks_lookup_depot` now delegate to
  the real hook modules. `hooks_probe_manifest` and
  `hooks_request_manifest_code` remain stubbed until 6e.

**Verified:**
- `K_INJECTED_PACKAGE_ID == 0`, `K_INJECTED_PKG_ACCESS_TOKEN ==
  10_660_652_434_190_618_804` (matches the C++ literal).
- `get_owned_depots` starts `(false, [])`.
- `License` struct round-trips.
- `join_ids` handles a null/zero-length input.
- `lookup_depot` on an empty registry returns `None`, and after an
  insert returns the recorded `(app_id, gid)` pair.
- `proto/steam_messages.proto` compiles cleanly under `prost-build`
  0.13. The generated `steam_messages.rs` is included by
  `src/steam/proto.rs`. Verified by `cargo test --lib` compiling the
  crate with the generated module present.

**Tests:** 6 new (package 4, manifest 2). Running total: 260.

**Still stubbed (pending 6d-6e):**
- `Hooks_IPC`, `Hooks_IPC_ISteamUser`, `Hooks_IPC_ISteamUtils` (6d).
  `IPCMessages.gen.h` is now in hand and the layouts are known.
- `Hooks_NetPacket` (6e).
- `bridge::hooks_probe_manifest` and
  `bridge::hooks_request_manifest_code` — both become real when
  `Hooks_NetPacket::ManifestProbe` lands in 6e.
- `bridge::appticket_write_ownership_ticket` /
  `appticket_write_encrypted_ticket` still return `false`. `Hooks_Package`
  and `Hooks_Decryption` do not depend on them; the fix is a two-line
  delegation to `utils::tickets::app_ticket::write_*` and will land in a
  follow-up patch since it was not part of 6c's scope.
- `dllmain`'s init thread — deferred to 6g with the file watchers.
- `Pipe/` — deferred to 6f.

**Known behavioural divergences:** no new entries.

### Batch 06b.4 - fix extract_depot_id_no_prefix test

**Fixed:**
- `src/hook/decryption.rs` test `extract_depot_id_no_prefix` asserted
  `Some(730)` for the input `730\DecryptionKey`, which has no backslash
  before the number. The C++ extractor uses
  `name.find_last_of("\\", last - 1)` to locate the backslash
  immediately before the depot id; when there is no such backslash it
  returns `npos` and skips the override path. The Rust port matches that
  behaviour and returns `None`. The test expectation was wrong; the
  function was correct. Test now expects `None`, with a comment
  explaining the C++ precedent.

**Tests:** no net change (existing test body corrected).

### Batch 06b.3 - restore steamui.rs after 6b.2 corrupted it

**Fixed:**
- `src/hook/steamui.rs` - the 6b.2 patch script used a PowerShell
  array-of-arrays that PowerShell flattened when the outer array
  contained a single element. The loop received two plain strings
  instead of one pair, and `$pair[0]` on a string returns its first
  character, so the substitution became a literal `C` -> `A` replace
  across the whole file. `steamui.rs` is restored from the 6b source
  with the intended `CAPTURED_GET_APP_BY_I_D` fix applied.
- `src/hook/misc.rs` was unaffected - its substitution list had two
  inner arrays, so PowerShell preserved the nesting and applied both
  substitutions correctly.

**Note for future patch scripts:** in PowerShell, `@( @('a','b') )`
does *not* produce a one-element array whose element is a two-element
array. The inner `@()` is unwrapped and the result is `@('a','b')`.
Use a flat list of hashtables or prefix the inner array with `,` to
force the nesting.

### Batch 06b.2 - fix paste slot-name references in misc/steamui

**Fixed:**
- `src/hook/misc.rs` and `src/hook/steamui.rs` - three manual slot
  references used names that do not match what `paste` actually emits.
  `paste`'s `:snake:upper` transformation splits runs of capital
  letters on each boundary, so `ID` becomes `I_D` and `CUtl` becomes
  `C_UTL`. The macros themselves (which always go through `paste!`)
  were correct; only the three `unsafe { SLOT.load() }` sites were
  wrong.
  * `CAPTURED_GET_APP_ID_FOR_CURRENT_PIPE` -> `CAPTURED_GET_APP_I_D_FOR_CURRENT_PIPE`
  * `O_CUTL_BUFFER_ENSURE_CAPACITY`        -> `O_C_UTL_BUFFER_ENSURE_CAPACITY`
  * `CAPTURED_GET_APP_BY_ID`               -> `CAPTURED_GET_APP_BY_I_D`

**Caveat for future hook code:** any manual reference to a
macro-generated slot name must use `paste`'s actual algorithm, not the
name you would write by hand. The `capture_ready!` / `install_hook_c!`
families are unaffected because they invoke the same paste machinery
that declares the slots. If a future hook needs a direct slot
reference and the derived name is unclear, split the identifier out of
the macro and reference it explicitly instead of hand-computing the
paste output.

### Batch 06b.1 - fix `$hmod` typo in appended arm_int3 macros

**Fixed:**
- `src/hook/macros.rs` - the `__arm_int3_c!` and `__arm_int3_u!` macros
  appended by 6b passed `$hmod` (a dollar-prefixed identifier that is
  not a metavariable of the enclosing macro) into `__arm_int3_step!`
  instead of the local variable `hmod`. rustc rejected it with
  `no rules expected $`. The variable name in the inner call is now
  `hmod`, matching the pattern already used by `__arm_capture_c!` and
  `__arm_capture_u!` from 6a.

### Batch 06b - Hooks: Misc + SteamUI + Decryption

**Source files ported:** `Hook/Hooks_Misc.h`, `Hook/Hooks_Misc.cpp`,
`Hook/Hooks_SteamUI.h`, `Hook/Hooks_SteamUI.cpp`,
`Hook/Hooks_Decryption.h`, `Hook/Hooks_Decryption.cpp`.

**Added:**
- `src/hook/misc.rs` - `Hooks_Misc`. Captures `GetAppIDForCurrentPipe`
  and `GetAppDataFromAppInfo`, arms a persistent int3 on
  `SpawnProcess` for OnlineFix detection and appid rewriting, and hooks
  `OptedInMask` / `BuildSpawnEnvBlock` to redirect the real appid in
  place of the -onlinefix shim. Public API: `resolve_app_id`,
  `is_onlinefix_active`, `notify_networking_sockets_used`,
  `should_report_onlinefix_app_id`, `ensure_buffer_capacity`,
  `get_game_name_by_app_id`.
- `src/hook/steamui.rs` - `Hooks_SteamUI`. Captures `GetAppByID` and
  `MarkAppChange`. Hooks `FillInAppOverview` (tracks active downloads,
  stamps PurchasedTime), `BuildCompleteAppOverviewChange` (pass-through,
  see divergences), and `CSteamUIAppControllerRunFrame` (drains the
  deferred-removal queue on the UI thread). Public API:
  `queue_removal`, `cancel_removal`, `active_download_count`.
- `src/hook/decryption.rs` - `Hooks_Decryption`. Hooks
  `ConfigStore::GetBinary` to serve Lua-provided depot decryption keys,
  and exposes `get_cache_app_ownership_ticket(app_id)` which reads
  `apptickets\<app_id>` from the captured local config store.
- `arm_int3_c!` / `arm_int3_u!` macros appended to
  `src/hook/macros.rs`. These register a persistent or one-shot int3
  site with a custom on-hit callback, mirroring the C++ `ARM_INT3`
  family. Needed by `Hooks_Misc::install` for the `SpawnProcess` trap.

**Changed:**
- `src/hook/mod.rs` - declares `misc`, `steamui`, `decryption`.
- `src/hook/framework.rs` - `install_steamclient_hooks` /
  `uninstall_steamclient_hooks` now include `decryption::*` and
  `misc::*`. `install_steamui_hooks` / `uninstall_steamui_hooks` now
  call `steamui::install` / `steamui::uninstall`.
- `src/bridge.rs` - `hooks_get_cache_app_ownership_ticket` now
  delegates to `hook::decryption::get_cache_app_ownership_ticket`
  instead of returning `Vec::new()`. The forged-appticket path in
  `utils::tickets::app_ticket` now has a real source.

**Known behavioural divergences (see STUBS.md):**
- `hook::steamui::BuildCompleteAppOverviewChange` installs and
  passes through, but does **not** re-inject our `removed_appid` set
  into the rebuilt overview. The C++ calls Steam's protobuf
  `RepeatedField<uint32>::Add` helper against a message created by the
  C++ protobuf runtime; prost uses a different in-memory layout for
  `repeated uint32`, so that helper cannot be called against
  prost-generated types. Fix path: resolve `RepeatedFieldUint32_Add`
  via `PatternLoader` and call it from Rust with a pointer to whatever
  Steam hands us. Deferred to a follow-up patch.
  *User-visible consequence:* a removed game can reappear in the
  library after a full library rebuild (rare — typically only on
  profile corruption, cache clear, or fresh login). Incremental
  updates are unaffected.
- `Hooks_Misc::GetOrAddAppData` is ported as a declaration only; the
  install line is commented out in the C++ and remains so. The body
  is not ported because the hook never installs.
- `HookManager.cpp` calls `Hooks_KeyValues::Install()` in a commented
  line; this port matches. `keyvalues::install()` exists and works
  when called.

**Verified:**
- `try_extract_depot_id` parses a well-formed
  `"...\<digits>\DecryptionKey"` key, rejects missing suffix, rejects
  non-numeric id, and rejects a key with no backslash before the id
  tail.
- `should_report_onlinefix_app_id` is `false` before any
  `SpawnProcess` hit.
- `is_onlinefix_active` is `false` initially.
- `get_app_id_for_current_pipe_wrap` returns 0 when the capture has
  not fired (no pointer dereference happens).
- `queue_removal` / `cancel_removal` round-trip: queue two, cancel
  one, leave one; cancel a non-queued id is a no-op.
- `active_download_count` is 0 on a clean registry.

**Tests:** 11 new (decryption 5, misc 4, steamui 2). Running total:
254.

**Still stubbed (pending 6c-6e):**
- `Hooks_Package`, `Hooks_Manifest` (6c).
- `Hooks_IPC`, `Hooks_IPC_ISteamUser`, `Hooks_IPC_ISteamUtils` (6d).
- `Hooks_NetPacket` (6e).
- `build.rs` — `prost-build` wiring lands in 6c.
- `dllmain`'s init thread — deferred to 6g with the file watchers.
- `Pipe/` — deferred to 6f.

### Batch 06a.1 - veh_common fixes

**Fixed:**
- `src/hook/veh_common.rs` - two compile errors in the 6a version:
  * `static HANDLER: RwLock<Option<trap::HandlerHandle>>` did not
    satisfy `Sync`, because `HandlerHandle` is `*mut c_void`. The
    handler is now stored as `usize` and cast back at the two use
    sites.
  * `arm()` moved `site` into the registry and then read
    `site.target` three lines later. The target is now read into a
    local before the move.

**Added:**
- One test (`handler_slot_is_usize_and_send_sync`) that is a
  compile-time assertion the handler storage is `Sync`. If a future
  edit reverts the type to a raw pointer, the test stops compiling.

**Verified:** the crate builds and `cargo test --lib` passes with the
hook tests included.

### Batch 06a - Hook framework + small hooks

**Source files ported:** `Hook/HookMacros.h`, `Utils/HookSupport/VehCommon.h`,
`Utils/HookSupport/VehCommon.cpp`, `Hook/PendingAPICalls.h`,
`Hook/PendingAPICalls.cpp`, `Hook/Hooks_CallBack.h`, `Hook/Hooks_CallBack.cpp`,
`Hook/Hooks_KeyValues.h`, `Hook/Hooks_KeyValues.cpp`,
`Hook/HookManager.h` (partial — the two orchestrators cover only 6a's
modules).

**Added:**
- `src/hook/mod.rs` - module tree.
- `src/hook/macros.rs` - the `hook_fn!`, `resolve_fn!`, `capture_this_fn!`
  family plus the `install_hook_*!`, `uninstall_hook!`, `resolve_*!`,
  `arm_capture_*!`, `capture_ready!` macros. Uses the `paste` crate for
  identifier concatenation; produces the same shape as the C++ macros:
  `O_<NAME>` slot, `hk_<Name>` hook function, `o_<Name>(...)` caller.
  `DetourAttach` overwrites the `O_*` slot with the trampoline on attach,
  matching the C++ `o##name` semantics exactly.
- `src/hook/veh_common.rs` - `Int3Site`, `arm`, `on_breakpoint`,
  `on_single_step`, `disarm_all`, `remove_handler`, `capture_rcx_to`,
  and the two RIP predicates (`is_at`, `is_post_int3_step`).
- `src/hook/framework.rs` - `Transaction` RAII guard, and the module-level
  `install_steamclient_hooks` / `uninstall_steamclient_hooks` /
  `install_steamui_hooks` / `uninstall_steamui_hooks` orchestrators
  (6a scope only).
- `src/hook/pending_api_calls.rs` - `record_encrypted_ticket` /
  `take_encrypted_ticket`, a `Mutex<HashMap<SteamAPICall_t, AppId_t>>`.
- `src/hook/callback.rs` - `SendCallbackToPipe` hook, pass-through for now.
- `src/hook/keyvalues.rs` - `FindOrCreateKey` resolve and
  `ReadAsBinary` hook, both pass-through. `install()` is not called from
  the orchestrator, matching the commented-out line in
  `HookManager.cpp`.

**Changed:**
- `Cargo.toml` - added `paste = "1"`.
- `src/dllmain.rs` - `CLIENT_HMODULE` and `UI_HMODULE` atomics with
  `set_*` / `*_hmodule` accessors, plus `K_ONLINE_FIX_APPID = 480`.

**Design notes:**
- The C++ `o##name` slot is *both* the pattern-resolved target address
  and the Detours trampoline slot. `DetourAttach` overwrites it with the
  trampoline. Rust's `AtomicPtr` would model this as UB in the strict
  sense (Detours writes through the raw pointer, not the atomic API), so
  the macro layer uses a small `Slot` newtype around
  `UnsafeCell<*mut c_void>` instead. The `Send`/`Sync` impls rely on
  Detours' guarantee that its commit path suspends every other thread
  before it writes.
- The macros are `#[macro_export]` so call sites in submodules can
  `use crate::{hook_fn, install_hook_c, uninstall_hook};`. The impl
  helpers (`__install_hook_step!`, `__arm_capture_step!`, etc.) carry
  `#[doc(hidden)]` and are considered crate-private in spirit.
- `VehCommon` uses a `parking_lot::RwLock<Vec<Int3Site>>`. The VEH
  handler takes a blocking read; installers take a write. No deadlock in
  practice because installers never wait on VEH-running threads. This
  matches the C++ contract — the C++ uses a plain `std::vector` with no
  synchronisation, so this is strictly safer.
- `platform::trap::Context::argument` already handles x64 fastcall args
  1-4 via RCX/RDX/R8/R9 and 5+ via `[RSP + 8*N]`, so no changes were
  needed in the trap module.

**Verified:**
- `record_encrypted_ticket` rejects call=0 and appid=0;
  `take_encrypted_ticket` is consuming and overwrites the previous value
  for a repeated key.
- `Slot` starts null, round-trips a pointer, and its `as_ptr()`
  addresses the underlying storage (matching what Detours expects).
- `is_at` and `is_post_int3_step` boundaries match the C++ predicates,
  including the exact-match exclusion on `is_post_int3_step`.
- `disarm_all` is safe on an empty registry.
- `set_client_hmodule` / `client_hmodule` round-trip.
- `K_ONLINE_FIX_APPID == 480`.

**Tests:** 12 new (`macros` 3, `veh_common` 4, `pending_api_calls` 5, `dllmain` 3, minus 3 that were converted to non-asserting form - net 12).

**Still stubbed (pending 6b-6e):**
- Everything in `bridge::hooks_*` (unchanged).
- `Hooks_Misc`, `Hooks_SteamUI`, `Hooks_Decryption`, `Hooks_Package`,
  `Hooks_Manifest`, `Hooks_IPC*`, `Hooks_NetPacket`.
- `dllmain`'s init thread — deferred to 6g with the file watchers.
- `build.rs` — `prost-build` wiring lands in 6c.

### Batch 06a-prep - Platform: real Detours backend

**Source files ported:** `OSTPlatform/Windows/Detour.cpp` (finalized),
`OSTPlatform/include/Detour.h` (FFI surface).

The Detours backend is now real. The `detour` module no longer returns
`false` unconditionally; `begin_transaction` / `commit_transaction` /
`attach` / `detach` all call into Microsoft Detours through a small
`extern "system"` FFI declared in `platform::windows::detours_ffi`.

**Added:**
- `src/platform/windows/detours_ffi.rs` - raw bindings to the seven
  Detours C entry points (`DetourTransactionBegin`,
  `DetourTransactionAbort`, `DetourTransactionCommit`,
  `DetourUpdateThread`, `DetourAttach`, `DetourDetach`,
  `DetourRestoreAfterWith`). Links against the vendored
  `third_party/Detours` static library.

**Changed:**
- `build.rs` - emits `cargo:rustc-link-search=native=.../lib.X64` and
  `cargo:rustc-link-lib=static=detours` on Windows targets. Fails the
  build with a clear message if `third_party/Detours/include/detours.h`
  or `third_party/Detours/lib.X64/detours.lib` is missing.
- `src/platform/detour.rs` - real implementation. Thread-local state
  tracks whether a transaction is open and whether the calling thread
  has been enrolled. `begin_transaction` is idempotent per-thread;
  `attach` / `detach` enrol the calling thread via
  `DetourUpdateThread(GetCurrentThread())` the first time they run in
  a transaction, matching the C++ `Detour.cpp` behaviour.
  `abort_transaction` is exposed as an additional helper for the hook
  layer to unwind a partial attach sequence.
- `src/platform/windows/mod.rs` - declares `pub mod detours_ffi`
  behind `#[cfg(windows)]`.

**Fixed:**
- `Cargo.toml` - added the `Win32_UI_Shell` feature to `windows-sys`.
  Batch 05's `tokeer::bridge::redeem` calls `ShellExecuteA` when the
  user accepts the launch prompt, but the feature was never enabled,
  so the crate did not compile. This was latent: the code shipped in
  the tree without a successful `cargo build` behind it.
- `src/platform/dynamic_library.rs` - `ModuleHandle` now has explicit
  `unsafe impl Send` and `unsafe impl Sync`. The underlying `HMODULE`
  is process-global and thread-agnostic; the default negative impls
  only existed because the field is `*mut c_void`. Batch 05's
  `cloud_redirect::host` stores a `ModuleHandle` in a `Mutex<...>`
  static, which requires `Send`.

**Verified:**
- `begin_transaction` returns true; `commit_transaction` returns true
  on an empty transaction; both round-trip through the thread-local
  state on x64.
- `begin_transaction` is idempotent within a thread.
- `commit_transaction` without an open transaction returns false and
  logs `LOG_PLATFORM_WARN`.
- `abort_transaction` clears state and is rejected outside a
  transaction.
- `attach` and `detach` with null arguments return false without
  crossing the FFI boundary.
- `attach` and `detach` outside a transaction return false without
  crossing the FFI boundary.
- `ModuleHandle` satisfies `Send + Sync` (compile-time assertion).
- On non-Windows targets, all four functions return false and log,
  matching the previous stub behaviour for the fallback target.

**Tests:** 10 new (`detour`).

**Known limits (not stubs, real behavioural notes):**
- x64 only. The lazy thread-enrolment pattern relies on
  `DetourTransactionCommit` suspending and updating every other
  thread, which is x64-specific behaviour. On x86 every thread would
  need to be enrolled explicitly via `DetourUpdateThread` before
  commit; the hook layer would need a thread-walk. The crate targets
  x64, so this is not exercised.
- The vendored `third_party/Detours` tree is a build prerequisite.
  `scripts\compile.bat` and `build.rs` both fail with clear instructions if
  it is missing.
- `DetourRestoreAfterWith` is declared but not yet called. It belongs
  in `dllmain.rs` once that module is written; the current stub does
  not load a DLL.

### Batch 05 - Utils: Tokeer + CloudRedirect

**Source files ported:** `TokeerBridge.cpp`, `TokeerBridge.h`,
`TokeerUri.cpp`, `CloudRedirectHost.cpp`, `CloudRedirectHost.h`.

**Deliberately deferred:** `AppUpdater.cpp`, `AppUpdater.h`. Not on
the critical path; guarded by `#ifdef OST_ENABLE_UPDATER` in the C++
build. See `STUBS.md` section "Deliberately not ported."

**Added:**
- `src/utils/tokeer/bridge.rs` - `redeem` (POST to
  `<base>/drm/redeem`, write tickets, prompt launch), `handle_uri`
  (parse `bst://<action>/<arg>`), `register_uri_scheme` (HKCU class
  registration). Base URL from `option_env!("OST_TOKEER_URL")` with
  default `https://luastools.xyz`.
- `src/utils/tokeer/uri.rs` - the `TokeerUri` export. On Windows,
  `#[no_mangle] pub extern "system"`.
- `src/utils/cloud_redirect/host.rs` - the `CR_*` ABI declared as
  `extern "C" fn` types, `initialize` loads `cloud_redirect.dll` and
  resolves exports, plus wrappers the NetPacket hook will use:
  `is_active`, `is_app`, `handle_cloud_rpc`, `set_account_id`,
  `notify_app_running`, `notify_stats_stored`, `get_achievements`,
  `sync_app_set`, `shutdown`.
- `src/utils/tokeer/mod.rs`, `src/utils/cloud_redirect/mod.rs`,
  updated `src/utils/mod.rs`.
- `STUBS.md` - "Deliberately not ported" section.

**Design notes:**
- `option_env!("OST_TOKEER_URL")` mirrors the C++ `#define`: empty
  at build time means the default URL.
- `resolve_symbol` uses `transmute_copy` from `*mut c_void` to the
  typed function pointer. Every CR signature is `extern "C"`.
- `handle_cloud_rpc` returns `Option<(usize, i32)>` so the caller
  gets both the response length and the eresult.

**Verified:**
- `hex_to_bytes` round-trips, rejects odd length.
- `json_string` finds keys, rejects null values.
- `json_true` distinguishes true from false.
- `resolve_library_path` handles bare name, absolute path, relative
  path.
- `is_active` gates `is_app` for uninitialized state.

**Tests:** 10 new (tokeer 6, cloud_redirect 4).

**Still deferred:**
- `AppUpdater` - self-updater, skipping.
- `dwmapi`, `xinput1_4` - proxy DLLs, waiting on loader-hijack batch.
- Wiring `register_uri_scheme` and `cloud_redirect::initialize` into
  `dllmain.rs` - both are ready; `dllmain` is still a stub.

### Batch 04.1 - Append missing bridge stub

**Fixed:**
- src/bridge.rs - the previous patch tried to insert
  hooks_get_cache_app_ownership_ticket before a comment anchor
  containing Unicode box-drawing characters. .Replace() silently
  no-op'd because the anchor string did not match byte-for-byte
  across the batch â†’ PowerShell â†’ file chain. The function is now
  appended to the end of the file unconditionally.

### Batch 04 - Utils: Tickets

**Source files ported:** `AppTicket.cpp`, `AppTicket.h`,
`LegacyCDKey.cpp`, `LegacyCDKey.h`, `EticketClient.cpp`,
`EticketClient.h`.

**Added:**
- `src/utils/tickets/app_ticket.rs` - `AppOwnershipTicket` struct,
  `get_app_ownership_ticket_from_credential_store` (reads from HKCU),
  `get_encrypted_ticket_from_credential_store`, the local-ticket
  forging path (source app 7, strip 128-byte sig, insert 4-byte appid,
  re-append), `write_app_ownership_ticket`, `write_encrypted_ticket`,
  `write_steam_id`, `extract_steam_id_from_ticket_bytes`, and
  `get_spoof_steam_id`. All credential-store calls delegate to
  `platform::steam_credential_store`, which is real since batch 2g.
- `src/utils/tickets/legacy_cd_key.rs` - `resolve(app_id, account_id)`
  returning `Option<String>`. Checks the Lua override first, then
  synthesizes a Crockford-base32 key seeded by FNV-1a of app+account.
- `src/utils/tickets/eticket_client.rs` - on-demand backend POST. URL
  resolution: `lua_config::get_eticket_url()`, then
  `option_env!("OST_ETICKET_URL")`, empty means disabled. Per-app
  ticket cache with SteamID-change eviction. Sticky no-owner set on
  409 without `"foreign_account":true`. Flat-JSON field extraction
  matching the C++ manual parser.

**Changed:**
- `src/bridge.rs` - added `hooks_get_cache_app_ownership_ticket`
  stub (returns `Vec::new()`). Real implementation lands with the
  Hook batch.
- `src/utils/mod.rs` - declares `pub mod tickets`.
- `STUBS.md` - new bridge row for the above.

**Verified:**
- Crockford alphabet contains only `0-9` and `A-Z` minus `ILOU`.
  Synthesized keys are 19 chars, `XXXX-XXXX-XXXX-XXXX`, deterministic
  on `(app_id, account_id)`, differ when either input differs.
- `extract_steam_id_from_ticket_bytes` returns 0 for short tickets,
  reads `[8..16]` as LE u64.
- `to_hex` / `from_hex` round-trip; odd-length and empty inputs
  rejected.
- `extract_string_field` finds values, rejects null, ignores missing
  keys.
- Credential-store path extracts the signature offset from the first
  u32 of the ticket header (matches the C++ misread).

**Known limits (not stubs, real behavioral notes):**
- The forging path calls `bridge::hooks_get_cache_app_ownership_ticket`,
  which returns empty until the Hook batch lands. So
  `AppTicketSource::ForgeOnly` and the fallback branch of
  `CredentialStoreThenForge` will fail with "no source appticket"
  until then. The credential-store path (which is what most titles
  use) works today.
- `EticketClient` is a no-op unless `OST_ETICKET_URL` is set at build
  time or `seteticketurl()` is called from Lua. This is by design:
  no deployment's backend is baked into the source.

**Tests:** 14 new (app_ticket: 3, legacy_cd_key: 5, eticket_client: 6).

### Batch 03d.2 - mlua call turbofish + u64 widening

**Fixed:**
- lua_config.rs - mlua 0.9's Function::call takes two generics
  (A: IntoLuaMulti, R: FromLuaMulti). The turbofish on the two new
  manifest-code callbacks was ::<Value>; corrected to ::<_, Value>
  so the argument type is inferred and only the return type is
  explicit.
- manifest_client.rs - call_manifest_fetch_code_ex takes u64 for
  pp_id and depot_id (matching the C++ signature which uses
  uint64_t), but the call site passed u32. Widened with s u64.

### Batch 03d.1 - Add Lua manifest-code callbacks

**Fixed:**
- lua_config.rs - added call_manifest_fetch_code(gid) and
  call_manifest_fetch_code_ex(app_id, depot_id, gid), both present
  in the C++ LuaConfig.cpp but omitted from the 3c port. Each locks
  the Lua state, looks up the named global, calls it with the
  appropriate arity, and extracts a u64 from either an integer
  return or a decimal-string return. Returns None on any failure:
  no state, missing function, Lua error, 
il, or unparseable value.

**Impact:** manifest_client::fetch_manifest_request_code can now
reach the Lua callbacks it was written to call. This was blocking
compilation of steam_metadata/manifest_client.rs.

### Batch 03d - Utils: SteamMetadata

**Source files ported:** all 9 under `Utils/SteamMetadata/`.

**Added:**
- `steam_diagnostics.rs` - SHA-256 of the two Steam DLLs, build-id
  read via `GetBootstrapperVersion`, `show_warning` on a detached
  thread.
- `stats_client.rs` - HTTP GET to `stats.opensteamtool.com/<appid>`,
  parses a decimal integer, caches per app.
- `mirror.rs` - three-mirror chain for `updates` branch assets.
- `remote_toml.rs` - SHA-hash the DLL, expand `{channel}/{component}/{sha256}`
  templates, try each mirror in order, cache the body, fall back to
  cache on remote failure.
- `pattern_loader.rs` - parse the pattern TOML into a
  `HashMap<usize, HashMap<u32, PatternEntry>>` keyed by module
  pointer, `find_pattern` resolves via RVA first then by scanning
  module memory, `report_missing_functions` surfaces one popup.
- `ipc_loader.rs` - parse the IPC TOML into a two-index registry
  (by-id, by-name), full port of the C++ lookup semantics.
- `manifest_client.rs` - the three-provider table with
  gid-only and app/depot/gid URL shapes, `set_provider`,
  `active_provider_name`, Lua-first then HTTP fallback.
- `manifest_cache.rs` - archive fetch into `<steam>/depotcache`,
  negative-cache TTL on 404, atomic write via tempfile + rename.
- `manifest_donor.rs` - wanted-list fetch, mint loop,
  `submit_captured_code` on a detached thread.

**Bridge updates:**
- `manifest_set_provider`, `manifest_active_provider_name`, and
  `stats_fetch_stat_steam_id` now delegate to the new modules.
- New stubs for Hook-batch dependencies: `hooks_get_owned_depots`,
  `hooks_lookup_depot`, `hooks_request_manifest_code`.

**Design notes:**
- `ModuleHandle` does not implement `Hash`; the pattern and IPC maps
  are keyed on the raw pointer as `usize`.
- `ManifestDonor::submit_captured_code` uses `std::thread::spawn`
  instead of `platform::thread::start_detached` (the latter takes a
  `u32`-returning closure; spawn is a simpler fit here).
- `ManifestCache::write_atomic` uses `std::fs::rename` for the
  atomic swap. Matches the C++ `MoveFileExA` semantics on the same
  volume.

**Tests:** 12 new (`stats_client` zero-appid, `mirror` expand,
`remote_toml` templates, `pattern_loader` sig parse + TOML extract,
`ipc_loader` interface name lookup, `manifest_client` URL formatting +
SteamRun JSON, `manifest_cache` magic check, `manifest_donor` line
parser).

**Not yet ported / missing:**
- `ManifestDonor::run_cycle` sees zero owned depots because
  `hooks_get_owned_depots` is stubbed; the mint loop does nothing.
- `ManifestClient` Lua paths still call through stubs in
  `lua_config.rs`.
- Everything else in `Utils/` — Tickets, CloudRedirect, Tokeer, Update,
  the two file watchers.

### Batch 03c.2 - Fix borrow-of-st.current_file in lua_config

**Fixed:**
- lua_config.rs - st.file_depots.entry(st.current_file.clone()) and
  the equivalent on ile_manifest_overrides borrowed st immutably
  inside a mutable entry() call. Hoisted st.current_file.clone() into
  a local first. Same fix in both places.


### Batch 03a-c.1 - Fix compile errors from 3a/3b/3c

**Fixed:**
- `build.rs` - emit macros without `#[macro_export]` and include at
  the crate root in `lib.rs` (Rust issue #52234 disallows referring
  to `macro_export` macros from `include!`-generated content via
  absolute paths). Module names uppercase: `LOG_PACKAGE_INFO`, not
  `LOG_Package_INFO`.
- `lib.rs` - `include!(OUT_DIR/log_macros.rs)` moved to crate root
  before module declarations. Rust's textual macro scoping then makes
  every `LOG_*` macro visible in every subsequent module.
- `logging/mod.rs` - removed the `include!` (now in `lib.rs`).
- `config.rs`, `lua_config.rs` - `crate::LOG_WARN!` replaced with
  unqualified `LOG_WARN!`.
- `lua_config.rs` - every public query that previously read
  `state.store.lock().unwrap().field` as the final expression now
  binds the result to a local first. The temporary `MutexGuard` in
  the expression previously outlived the outer `guard`, causing
  E0597.
- `lua_config.rs` - `load_std_libs` -> `load_from_std_lib` (mlua 0.9
  renamed the method).
- `lua_config.rs` - `.get::<Value>("name")` -> `.get("name")` with
  the return type inferred from `let f: Result<Value, _>` (mlua 0.9
  `Table::get` takes two generics: `K: IntoLua, V: FromLua`).
- `lua_config.rs` - removed dead `_url_placeholder`; http stubs now
  return `(Value::Nil, Value::String("HTTP request failed"))`.
- Removed `mut` from unused bindings in `config.rs` and
  `merge_watch_dirs`.

### Batch 03c - Utils: Config

**Source files ported:** `Utils/Config/Config.cpp`, `Config.h`,
`LuaConfig.cpp`, `LuaConfig.h`.

**Not yet ported:**
- `Utils/Config/LuaFileWatcher.cpp`
- `Utils/Config/ConfigFileWatcher.cpp`

Both depend on `platform::directory_watch::Watch` which is a stub
until the Windows backend for DirectoryWatch gets real
implementation. They will be ported alongside the CLI/dllmain
wiring once DirectoryWatch is live.

**Added:**
- `src/bridge.rs` - placeholders for cross-module functions that are
  called by Config/LuaConfig but belong to later batches. Every bridge
  has a real signature so the call site does not need to change when
  the real implementation lands. Currently stubbed:
  `ManifestClient::SetProvider`, `StatsClient::FetchStatSteamId`,
  `AppTicket::Write*`, `Hooks_Package::NotifyLicenseChanged`,
  `Hooks_NetPacket::ProbeManifest`, `CloudRedirectHost::SyncAppSet`.
- `src/utils/config/config.rs` - TOML config via serde. Schema
  mirrors the C++ walker. `[remote].url_template` accepts both the
  string form and the array form via `#[serde(untagged)]`. Donate
  range clamping is preserved.
- `src/utils/config/lua_config.rs` - Lua state via `mlua`
  (`lua54`, `vendored`). All the custom functions are registered:
  `addappid`, `addtoken`, `setlegacycdkey`, `addprocess`,
  `forcedenuvo`, `seteticketurl`, `setmanifestid`, `setappticket`,
  `seteticket`, `setstat`. `http_get`/`http_post` are stubs that
  return `nil` until the network layer lands.
  Per-file refcounting, manifest override rebuild, and mtime-based
  purchase time are ported.

**Simplified vs C++:**
- Lua file parsing runs the whole file rather than re-trying per
  line. The C++ does the per-line thing so partial multi-line
  scripts work in isolation; mlua handles multi-line input
  natively and the observable effect is the same.
- Case-insensitive global lookup (`__index` on `_G`) is not
  implemented. Functions are lowercase-only, matching the
  common convention. If a specific script relies on mixed-case
  Lua function names, we can add the metatable then.

**Tests:** Config: log-level parsing, default snapshot shape,
TOML known shape, url_template both forms. LuaConfig: hex decoder,
merge_watch_dirs dedupe. ~6 tests.

### Batch 03b - Utils: Support

**Source files ported:** `Utils/Support/FnvHash.h`, `Stopwatch.h`.

**Added:**
- `src/utils/support/fnv.rs` - `fnv1a_hash`, a `const fn` so it can
  be used in `const` contexts (matches the C++ `constexpr`).
  Verified against the values from `PatternLoader.cpp`'s
  `static_assert(Fnv1aHash("BBuildAndAsyncSendFrame") == 0x82428E37)`.
- `src/utils/support/mod.rs` - re-exports `Stopwatch` from
  `platform::stopwatch` (they are identical in C++).

**Tests:** FNV known values + the const-eval check. 2 tests.

### Batch 03a - Utils: Logging

**Source files ported:** `Utils/Logging/Log.cpp`, `Log.h`,
`LogModules.def`.

**Added:**
- `build.rs` - generates the per-module `LOG_*` macros at build time.
  Module list mirrors `LogModules.def`; adding a module means one
  entry in `build.rs` and one variant in `LogModule`.
- `src/utils/logging/`:
  * `level.rs` - `Level` enum, matching `Config::LogLevel` ordering.
  * `module.rs` - `LogModule` with `as_index` and `file_name`.
  * `logger.rs` - one `Logger` per module; writes to a `File` behind
    a `parking_lot::Mutex`, keeps its level in an `AtomicU8`,
    formats `[timestamp] [LEVEL] [tid=N] [file:line] message`.
  * `state.rs` - `OnceLock<State>` holding all loggers plus a global
    `RwLock<Level>`. `init_main` creates all files at Trace level;
    `apply_config_level` sets their levels.
  * `mod.rs` - the public API plus `include!(OUT_DIR/log_macros.rs)`.

**Not yet wired:**
- The host's `init` entry point (`dllmain.rs`) and the CLI
  (`main.rs`) still need to call `logging::init(...)`. That lands
  with the dllmain batch.
- `platform::logging` sink routes into `LogModule::Platform` but
  is only installed when `install_platform_log_sink` is called
  explicitly.

**Tests:** `Logger` writes and respects level; `Level` ordering;
`LogModule` index round-trip. ~5 tests.

### Batch 02g.1 - Platform: LSTATUS type + UnmapViewOfFile wrapper

**Fixed:**
- `steam_credential_store.rs` - `windows-sys` models `LSTATUS` as
  `WIN32_ERROR = u32`, not the `i32` the Win32 header names it. All
  helper signatures and the four error-code constants
  (`ERROR_SUCCESS`, `ERROR_FILE_NOT_FOUND`, `ERROR_PATH_NOT_FOUND`,
  `ERROR_NOT_FOUND`) are now `u32`, and `from_lstatus` takes `u32`.
- `handles.rs` - `UnmapViewOfFile` in `windows-sys` 0.59 takes a
  `MEMORY_MAPPED_VIEW_ADDRESS` wrapper struct, not a raw pointer.
  `UniqueMapView::reset` now constructs the wrapper before calling.

### Batch 02g - Platform: SteamCredentialStore (final platform sub-batch)

**Source files ported:** `OSTPlatform/Windows/SteamCredentialStore.cpp`.
Also completes `OSTPlatform/Windows/Handles.h` - the RAII close
functions now call into Win32.

**Added:**
- `src/platform/steam_credential_store.rs` - HKCU reads and writes:
  * `get_app_ticket` / `write_app_ticket` - REG_BINARY under
    `Software\Valve\Steam\Apps\<appId>\AppTicket`.
  * `get_e_ticket` / `write_e_ticket` - REG_BINARY under the same
    key, value name `ETicket`.
  * `get_steam_id` / `write_steam_id` - REG_SZ decimal under the
    same key. Reads parse the string as `u64`, treat empty/zero/
    unparseable as `NotFound`, and mirror the C++ "missing value is
    not an error" logic.
  * `get_active_user` - reads `ActiveUser` (REG_DWORD) and
    `Universe` (REG_SZ) from
    `Software\Valve\Steam\ActiveProcess`. Returns `(account_id,
    universe)`.

**Fixed:**
- `src/platform/windows/handles.rs` - the four RAII guards now
  actually close their resources:
  * `NullHandlePolicy::close` and `InvalidHandlePolicy::close`
    call `CloseHandle`.
  * `UniqueRegKey::reset` calls `RegCloseKey`.
  * `UniqueMapView::reset` calls `UnmapViewOfFile`.
  All three are gated `#[cfg(windows)]`; on other targets they
  stay no-ops so the crate still compiles everywhere.

**Changed:**
- `Cargo.toml` - added `Win32_System_Registry` feature to
  `windows-sys`.

**Test changes:**
- Tests in `handles.rs` that constructed fake non-null handles
  (`0x1000`, `0x4000`, `0x8000`) and let them drop are gone. With
  the real closes in place those tests would call the Win32 close
  on a bogus address, which is UB. The RAII structure is now
  tested exclusively via the test-only `SpyPolicy`. Tests that use
  null/empty values still exist; tests that need a non-null handle
  only for metadata assertions call `std::mem::forget` at the end.

**Verified on Windows:**
- No unit tests touch the live HKCU hive. Reading or writing Steam
  credential keys from a test would corrupt user state. The
  functions are exercised through the CLI and integration tests
  instead.
- `Status` round-trips through `as_str()` and matches the C++
  string constants exactly.

**Platform layer complete.** With this batch, `src/platform/`
covers every file under `OSTPlatform/`. The only stubbed Windows
surface left is `Detour::attach` / `detach` (deferred to the hook
batch) and the four remaining trivial `Detour` transaction
functions.

### Batch 02f.2 - Platform: WinHTTP proxy nulls

**Fixed:**
- `http.rs` - `WINHTTP_NO_PROXY_NAME` and `WINHTTP_NO_PROXY_BYPASS` are
  also NULL-pointer constants that `windows-sys` does not re-export.
  Both are now defined locally alongside the other two. All four
  `WINHTTP_NO_*` constants now come from the module, not the crate.

### Batch 02f.1 - Platform: HTTP constant + Process helper fixes

**Fixed:**
- `http.rs` - `WINHTTP_NO_REFERER` and `WINHTTP_NO_ADDITIONAL_HEADERS`
  are NULL pointer constants that `windows-sys` does not re-export.
  Defined locally as `const *const u16 = std::ptr::null()`.
  `WINHTTP_NO_PROXY_NAME` and `WINHTTP_NO_PROXY_BYPASS` are correctly
  exported and were moved to their real import location in the
  `windows_sys::Win32::Networking::WinHttp` module (they were already
  there; the error was that I was importing the other two from the
  same module where they don't exist).
- `process.rs` - replaced `u16::to_ascii_uppercase` (which does not
  exist on the primitive) with a small `ascii_upper_u16(c)` helper
  that handles the ASCII range 0x61..=0x7A. Environment variable
  names are ASCII by convention; this matches Windows' case rules
  for those names.
- `process.rs` - removed a leftover `_touch` shim that called
  `GetCurrentProcess` outside an `unsafe` block. It was dead code
  from an earlier iteration.

### Batch 02f - Platform: PE, Http, DirectoryWatch, Process, RemoteProcess

**Source files ported:** `OSTPlatform/Windows/PE.cpp`,
`OSTPlatform/Windows/Http.cpp`,
`OSTPlatform/Windows/DirectoryWatch.cpp`,
`OSTPlatform/Windows/Process.cpp`,
`OSTPlatform/Windows/RemoteProcess.cpp`.

This is the largest single batch. All five modules are real
implementations on Windows; every one retains its non-Windows
fallback stubs.

**Added (PE):**
- Local PE structs (`ImageDosHeader`, `ImageFileHeader`,
  `ImageOptionalHeader32/64`, `ImageSectionHeader`,
  `ImageExportDirectory`). We do **not** use `windows-sys`'s
  `IMAGE_*` structs; they live in different feature-gated modules
  across crate versions and the format is stable enough to declare
  locally.
- `Image::new` parses the DOS header, NT signature, file header,
  optional header (both PE32 and PE32+), and section table. Every
  read uses `read_unaligned` from the byte buffer via a small
  `pod_at` helper.
- `rva_to_offset` / `raw_offset_to_rva` - section-based translation
  with the same clamping rules as the C++.
- `find_export` - walks the export directory's name/ordinal/function
  tables, returns `Export { rva, forwarder }`. Forwarder strings are
  detected and returned as-is.
- `read_raw_bytes` / `read_all_bytes` - file reads through
  `std::fs::File`.

**Added (Http):**
- Full WinHTTP: `WinHttpOpen`, `WinHttpConnect`,
  `WinHttpOpenRequest`, `WinHttpAddRequestHeaders`,
  `WinHttpSendRequest`, `WinHttpReceiveResponse`,
  `WinHttpQueryHeaders`, `WinHttpQueryDataAvailable`,
  `WinHttpReadData`, `WinHttpSetTimeouts`, `WinHttpSetOption`,
  `WinHttpCloseHandle`.
- `parse_url` handles `http://` / `https://`, host, optional port,
  and path component.
- gzip/deflate decompression requested via
  `WINHTTP_OPTION_DECOMPRESSION`; on older SDKs the option is
  silently ignored.
- `max_body_bytes` is enforced on read - the accumulator is checked
  after each chunk; reading stops when the cap is reached.

**Added (DirectoryWatch):**
- Real `Watch` handle around `CreateFileW` with
  `FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED`, plus an event
  created via `CreateEventW`.
- `ReadDirectoryChangesW` with `FILE_NOTIFY_CHANGE_FILE_NAME |
  FILE_NOTIFY_CHANGE_LAST_WRITE`. `ERROR_IO_PENDING` is treated as
  success; other errors abort.
- `drain` walks the `FILE_NOTIFY_INFORMATION` chain and converts
  each entry to a `Change { relative_path, action }`.
- `wait_any` uses `WaitForMultipleObjects` and translates the
  result index back to the caller's array index (skipping
  not-open watches).

**Added (Process):**
- `get_creation_time` via `GetProcessTimes`.
- `format_creation_time` computes Unix seconds from the FILETIME
  offset and formats with `chrono` (UTC).
- `get_image_path` via `QueryFullProcessImageNameW` with the
  buffer-growth retry on `ERROR_INSUFFICIENT_BUFFER`.
- `enumerate_modules` via `CreateToolhelp32Snapshot` and the
  `Module32FirstW` / `Module32NextW` pair.
- `launch_detached_hidden` via `CreateProcessW` with
  `CREATE_NO_WINDOW` and `CREATE_BREAKAWAY_FROM_JOB` (with a
  fallback to no-breakaway if the parent's job forbids it).
- PEB walking: `NtQueryInformationProcess` (for the PEB address),
  `NtQueryVirtualMemory` (for region sizes), and
  `ReadProcessMemory` for the actual reads. Both NT functions are
  resolved once via `GetModuleHandleW("ntdll.dll")` +
  `GetProcAddress`.
- `get_process_command_line` tries the WoW64 path first (32-bit
  target), falling back to the native PEB walk.
- `get_environment_variable_value` reads the environment block
  starting from the PEB, capped at 1 MiB, and matches `name=` on a
  case-insensitive basis, exactly like the C++.

**Added (RemoteProcess):**
- `get_architecture` uses `IsWow64Process`.
- `inject_library` performs the classic `LoadLibraryW` injection:
  `VirtualAllocEx` the wide path, `WriteProcessMemory`,
  `CreateRemoteThread` with the resolved `LoadLibraryW` address,
  `WaitForSingleObject(10s)`, `GetExitCodeThread`, then cleanup.
- `LoadLibraryW` is resolved by parsing the target's on-disk
  `kernel32.dll` with `crate::platform::pe::Image` and adding the
  module's remote base address - identical to the C++ strategy.
  Forwarders (e.g. to `kernelbase.dll`) are followed one level
  deep; ordinal-only forwarders are rejected.

**Verified on Windows:**
- PE parser successfully parses `kernel32.dll`: valid image, more
  than one section, `.text` present, `LoadLibraryW` export found.
- `rva_to_offset` / `raw_offset_to_rva` round-trip for
  `kernel32.dll`.
- `DirectoryWatch::open` succeeds on `%WINDIR%\System32` and
  `issue_read` queues cleanly.
- `get_creation_time(pid=current)` returns a non-zero value.
- `get_image_path(pid=current)` returns a non-empty path.
- `enumerate_modules(pid=current)` returns a list that includes
  `kernel32.dll`.
- `get_architecture(pid=current)` reports `X64` on a 64-bit build.
- `get_architecture(0xFFFFFFF0)` returns `Unknown` (OpenProcess
  fails as expected).

**Not verified in unit tests (by design):**
- Network access in `Http::execute`.
- Actual DLL injection into a live process.
- Command-line / environment reads on a foreign process; the
  current process is the only target a test can reliably inspect,
  and even that needs `PROCESS_VM_READ` on itself, which is
  normally granted but not guaranteed in hardened environments.

**Design decisions recorded:**
- **Local PE structs, not `windows-sys`.** See top of `pe.rs`.
- **Synchronous HTTP, no async.** Same shape as the C++; the async
  runtime in the crate is for the CLI tool, not for these calls.
- **`read_unaligned` for every PE field read.** PE sections are not
  guaranteed aligned in the header buffer; forming a reference
  would be UB.
- **PEB walking is `unsafe` and gated.** Every remote read is
  `ReadProcessMemory` (returns error, does not crash); every
  pointer is null-checked. Failure to resolve any step returns
  `None`, which callers treat as "no data".

**Not yet ported / missing:**
- `SteamCredentialStore` (sub-batch 2g).
- Real `Detour::attach` / `detach` (deferred to the hook batch).

### Batch 02e - Platform: Memory, Hash backend, ByteSearch

**Source files ported:** `OSTPlatform/Windows/Memory.cpp`,
`OSTPlatform/Windows/ByteSearch.cpp`. `Hash.cpp` already uses the
`sha2` crate (implemented in 2b) and required no change.

**Added (full implementations):**
- `src/platform/memory.rs`:
  * `get_module_image` - calls `GetModuleInformation` from PSAPI,
    returns `ModuleImage { base, size }`. The `Option<ModuleImage>`
    shape matches the C++ `std::optional<ModuleImage>`.
  * `write_executable_byte` - the full VirtualProtect(RWX) /
    write / VirtualProtect(restore) / FlushInstructionCache sequence.
    Returns false on any of the three failing.
  * `is_readable` - walks every region the range spans via
    `VirtualQuery`, with the same protection checks as the C++:
    `PAGE_NOACCESS` and bare `PAGE_EXECUTE` are excluded, and any
    range that steps into an uncommitted or guard region fails.
    The region-walk loop breaks if the range does not make forward
    progress, avoiding a spin on a corrupt region table.

**Added (real implementation, simplified read engine):**
- `src/platform/byte_search.rs`:
  * `find` - BMH scan, byte-for-byte port of the C++
    `FixedPatternScanner` (skip table filled with pattern size, then
    overwritten for all positions but the last).
  * `find_in_file` - routes through `find_in_file_range` with
    `u64::MAX` size, matching the C++ design.
  * `find_in_file_range` - sliding window with `pattern.len() - 1`
    byte overlap between chunks. Same boundary-straddle behaviour as
    the C++.

**Design decisions recorded:**
- **ByteSearch read path is synchronous.** The C++ uses overlapped
  (async) reads with double buffering so the scan of chunk i
  overlaps the read of chunk i+1. This port uses positioned
  `seek` + `read` calls. Results are identical; the port does not
  exploit disk/CPU concurrency. If throughput matters later
  (e.g. scanning a ~100 MB module during a ProtectionScan), swap
  the read loop for `windows-sys` `ReadFile` with `OVERLAPPED`;
  the public API will not change.
- **Hash backend unchanged.** The `sha2`-crate implementation
  produces byte-identical digests to the C++ BCrypt path and needs
  no `windows-sys` involvement. Keeping it portable means the
  `hash` module still compiles on non-Windows.
- **`is_readable` guards against wraparound.** The C++ checks
  `start > UINTPTR_MAX - bytes` before computing `end`. We do the
  same. Without it, a call with `addr = usize::MAX` and `bytes > 1`
  would wrap to a small `end` and the region walk would return
  `true` for an out-of-range address.

**Verified on Windows:**
- `get_module_image(kernel32.dll)` returns a non-null base and a
  non-zero size.
- `get_module_image(ModuleHandle::null())` returns `None`.
- `is_readable` returns true for a 16-byte stack array, false for
  a null pointer, and false for a zero-byte range.
- BMH `find` matches the C++ on: exact match at offset 0, match at
  the end, first-match-only semantics, single-byte needles, and
  partial-overlap cases (`aaab` -> `aab` at 1).
- File scan finds a pattern that straddles a 16-byte chunk boundary
  when the pattern is 6 bytes long and starts at offset 13.
- `find_in_file_range` respects `offset`, clamps `size` to EOF, and
  returns `None` for offset past EOF, for zero-size windows, and
  for a missing file.

**Not yet ported / missing:**
- `Http`, `DirectoryWatch`, `Process`, `RemoteProcess` (sub-batch 2f).
- `SteamCredentialStore` (sub-batch 2g).

### Batch 02d.2 - Platform: Win32 constant + thunk fixes

**Fixed:**
- `trap.rs` - `EXCEPTION_SINGLE_STEP` moved to `Win32::Foundation`
  (same module as `EXCEPTION_BREAKPOINT`). It is not re-exported from
  `Win32::System::Diagnostics::Debug` in `windows-sys` 0.59 even with
  `Win32_System_Kernel` enabled.
- `dynamic_library.rs` - added `get_current_directory_w_thunk` and
  `get_system_directory_w_thunk` to normalise the argument order.
  `GetCurrentDirectoryW` takes `(capacity, buffer)`; `GetSystemDirectoryW`
  takes `(buffer, capacity)`. The C++ header has the same two thunks
  for the same reason.

### Batch 02d.1 - Platform: Win32 feature gate fixes

**Fixed:**
- `Cargo.toml` - added `Win32_System_Kernel` feature. Without it,
  `AddVectoredExceptionHandler`, `RemoveVectoredExceptionHandler`,
  `CONTEXT`, `EXCEPTION_POINTERS`, and `EXCEPTION_SINGLE_STEP` are
  gated out by `windows-sys`, causing five compile errors in
  `trap.rs`.
- `dynamic_library.rs` - `GetSystemDirectoryW` imported from
  `Win32::System::SystemInformation` instead of
  `Win32::System::Environment`. `GetCurrentDirectoryW` stays where it
  was; the two live in different modules in `windows-sys` 0.59.
- `lib.rs` - added `#![allow(non_camel_case_types)]`. `windows-sys`
  and the NtAbi port both use ALL_CAPS type aliases (`ULONG_PTR`,
  `SIZE_T`, `PSIZE_T`, `HANDLE`, ...) that mirror the Win32 typedefs.
  Same rationale as the `non_snake_case` allow added in batch 01.2.

### Batch 02d - Platform: Win32 backends (DynamicLibrary, Thread, Dialog, Trap)

**Source files ported:** `OSTPlatform/Windows/DynamicLibrary.cpp`,
`OSTPlatform/Windows/Thread.cpp`, `OSTPlatform/Windows/Dialog.cpp`,
`OSTPlatform/Windows/Detour.cpp` (API only),
`OSTPlatform/Windows/Trap.cpp`.

**Added (full implementations):**
- `dynamic_library.rs` - real `LoadLibraryW` / `GetModuleHandleW` /
  `GetProcAddress` (by name and ordinal) / `GetModuleFileNameW` /
  `GetCurrentDirectoryW` / `GetSystemDirectoryW`. The path-buffer growth
  loop from the C++ is preserved (`GetModuleFileNameW` returns
  `len == capacity` when the buffer is too small; we double and retry up
  to 32 KiB).
- `thread.rs::current_native_thread_handle` - real `GetCurrentThread`.
  `start_detached` was already real; unchanged.
- `dialog.rs` - real `MessageBoxA` with `MB_OK | MB_ICONWARNING` and
  friends, plus `MB_TOPMOST` so the box stays visible above Steam's
  overlay. Return value of `IDYES` drives `show_confirm`.
- `trap.rs` - real VEH install. `AddVectoredExceptionHandler` receives a
  Rust `extern "system"` thunk that reads the current handler pointer
  from an `AtomicPtr`, converts the Win32 `EXCEPTION_POINTERS` into a
  `Context`, and translates `EXCEPTION_BREAKPOINT` / `EXCEPTION_SINGLE_STEP`
  into our `ExceptionKind`. Return value mapping is
  `true -> EXCEPTION_CONTINUE_EXECUTION (-1)`,
  `false -> EXCEPTION_CONTINUE_SEARCH (0)`.
  `Context::argument` reads `RCX`/`RDX`/`R8`/`R9` for args 1-4 and
  `read_unaligned` from `RSP + 8*index` for args 5+.
  `Context::enable_single_step` sets bit 8 (TF) of `EFlags`.

**Added (API only, stub body):**
- `detour.rs` - the transaction / attach / detach API surface is final.
  The body returns `false` everywhere until a dedicated batch adds
  either FFI to Microsoft Detours (requires a `build.rs` fetching and
  building the Detours repo) or a Rust port using the `retour` crate.
  Callers already check the return value (`HookMacros.h` sets
  `_ost_detour_transaction_ok_ = false` on failure), so returning false
  consistently is safe: no hooks install, nothing crashes.

**Changed:**
- `Cargo.toml` - trimmed `windows` crate (unused), kept `windows-sys`
  with an expanded feature set:
  `Win32_System_Environment`, `Win32_System_Diagnostics_Debug`,
  `Win32_System_SystemInformation`, `Win32_UI_WindowsAndMessaging`.
  `retour` removed from dependencies (its API shape did not match the
  C++ `Attach(void**, void*)` contract; a build-script-based Detours
  integration is the honest translation).

**Verified on Windows:**
- `LoadLibraryW("kernel32.dll")` returns a non-null handle.
- `GetModuleHandleW("kernel32.dll")` returns a non-null handle.
- `GetProcAddress(kernel32, "LoadLibraryW")` resolves.
- `GetProcAddress` for a nonexistent export returns null.
- `GetCurrentDirectoryW` and `GetSystemDirectoryW` return non-empty
  strings.
- `GetModuleFileNameW(NULL, ...)` returns the test runner's executable
  path.
- `AddVectoredExceptionHandler` returns a non-null handle, and
  `RemoveVectoredExceptionHandler` accepts it cleanly.

**Verified on non-Windows:**
- All tests still pass with the stub fallbacks. Module-level `#[cfg]`
  means the crate compiles identically on both targets.

**Not yet ported / missing:**
- Detours backend (deferred).
- `Memory`, `Hash` backend, `ByteSearch` backend, `PE`, `Http`,
  `DirectoryWatch`, `Process`, `RemoteProcess`,
  `SteamCredentialStore` (sub-batches 2e-2g).

### Batch 02c - Platform: private headers

**Source files ported:** `OSTPlatform/Windows/Handles.h`,
`OSTPlatform/Windows/NtAbi.h`.

**Added:**
- `src/platform/windows/handles.rs` - four RAII ownership primitives:
  * `UniqueHandleT<P: HandlePolicy>` - generic HANDLE guard; the C++
    two-policy design (`NullHandlePolicy` / `InvalidHandlePolicy`)
    maps directly to a Rust trait with two zero-sized implementors.
    Type aliases `UniqueHandle` and `UniqueFileHandle` preserve the
    C++ names.
  * `UniqueRegKey` - HKEY guard. `put()` returns `*mut HKEY` for
    out-parameter use with `RegCreateKeyExW`, matching the C++
    `HKEY* put()`.
  * `UniqueMapView` - memory-mapping view guard.
  * `ScopeExit<F>` - run-on-drop closure; `dismiss()` cancels.
- `src/platform/windows/nt_abi.rs` - the NtDLL ABI surface:
  * `ProcessInfoClass`, `MemoryInfoClass` enums with the same
    discriminants as the C++ (`Wow64Information = 26`).
  * `ProcessBasicInformation`, `Peb`, `Peb32`, `UnicodeString`,
    `UnicodeString32`, `RtlUserProcessParameters`,
    `RtlUserProcessParameters32` - all `#[repr(C)]` with
    `const _: () = assert!(...)` checks matching the C++
    `static_assert`s.
  * Function-pointer types `NtQueryInformationProcessFn` and
    `NtQueryVirtualMemoryFn` with `extern "system"` (i.e. `__stdcall`,
    which on x64 is the same as every other convention).
  * `nt_success(status: NTSTATUS) -> bool` - signed comparison,
    matching the C++ `status >= 0`.
- `src/platform/windows/mod.rs` - module declarations.
- `src/platform/mod.rs` - appended `pub mod windows;`.

**Design decisions recorded:**
- **Close functions are stubs.** `HandlePolicy::close` for both
  policies, `UniqueRegKey::Drop`, and `UniqueMapView::Drop` are
  no-ops until 2d wires in `CloseHandle` / `RegCloseKey` /
  `UnmapViewOfFile`. The RAII *structure* is fully enforced (moves,
  drops, and borrowck), only the OS effect is deferred. Tests use
  sentinel pointers and never real handles, so this is safe to run
  today.
- **Raw pointer types.** `HANDLE`, `HKEY`, `PVOID` are raw
  `*mut c_void` in this module, not the `windows` crate's newtypes.
  Sub-batch 2d will cast at the FFI boundary. Keeping them raw
  avoids a dependency on the `windows` crate in a module that is
  otherwise platform-agnostic.
- **Spy policy for tests.** A test-only `SpyPolicy` uses a
  `thread_local!` counter to verify `Drop`, `reset_to`, and
  double-reset semantics without needing real OS handles.

**Verified:**
- All six `static_assert` offset checks pass at compile time.
- `size_of::<ProcessBasicInformation>() == 48`, matching the x64
  layout.
- `nt_success(0xC0000001u32 as i32) == false`, confirming the signed
  comparison.
- `UniqueHandleT` closes the old handle exactly once when
  `reset_to` replaces it, and zero times on a null-to-null reset.
- `ScopeExit` fires the closure exactly once on drop; `dismiss`
  suppresses it.

**Not yet ported / missing:**
- Real close functions for all three RAII wrappers.
- `NtQueryInformationProcess` and `NtQueryVirtualMemory` bindings
  (added in 2f when `Process.cpp` is ported).

### Batch 02b - Platform: public headers

**Source files ported (types + stubs):** all 18 headers under
`OSTPlatform/include/`. Three (`Encoding.h`, `Log.h`, `Numbers.h`) were
already ported in 2a; the other 15 produce new modules here.

**Added (full implementations):**
- `src/platform/hash.rs` - `sha256_of_file`, `sha256_of_buffer` using
  the `sha2` crate. Byte-identical to the C++ BCrypt implementation;
  verified against known SHA-256 test vectors (empty string, `"abc"`,
  1 MiB of zeros). 64 KiB chunked file reads match the C++ loop.
- `src/platform/stopwatch.rs` - `Stopwatch` backed by `Instant`.
- `src/platform/thread.rs` - `start_detached` using `std::thread` and
  dropping the `JoinHandle`. `current_native_thread_handle` is a stub
  until 2d (needs `GetCurrentThread`).
- `src/platform/byte_search.rs` - `find` implemented with a naive scan.
  BMH is a drop-in replacement later; the API will not change.
- `src/platform/process.rs::is_system_module_path` - real
  implementation using `%WINDIR%`. Case- and separator-insensitive;
  matches `GetWindowsDirectoryW` on any normal Windows install.

**Added (types + stubs):**
- `detour.rs`, `dialog.rs`, `directory_watch.rs`, `dynamic_library.rs`,
  `http.rs`, `memory.rs`, `pe.rs`, `remote_process.rs`,
  `steam_credential_store.rs`, `trap.rs`. Every public function has
  its full signature; the bodies either return a "nothing happened"
  sentinel (Ok(false), None, Err(Status::Unsupported), etc.) or are
  no-ops.

**Design decisions recorded:**
- `ModuleHandle` is a `#[repr(transparent)]` newtype around
  `*mut c_void` rather than the `windows` crate's `HMODULE`. This
  keeps the type platform-agnostic while still being exactly
  pointer-sized. The Windows backend will cast freely.
- `Process::ModuleInfo` mirrors the C++ shape with both `path: String`
  and `native_path: PathBuf`. The C++ has the same redundancy; keeping
  it avoids restructuring call sites.
- `PE::ByteBuffer` uses `Vec<u8>` (zero-filled at allocation). The C++
  uses `std::make_unique_for_overwrite` (uninitialized). The semantic
  difference is a `memset` at allocation time. If profiling shows this
  matters for large module reads, we can swap to a `MaybeUninit<u8>`
  backing store without changing the public API.
- `Trap::Handler` is `fn(ExceptionKind, &mut Context) -> bool`, not
  `extern "C"`. The handler is invoked from our own Rust VEH trampoline,
  so no FFI ABI is needed. `Context::from_native` is the only `unsafe`
  entry point and it is documented as such.
- `Thread::NativeThreadHandle` stays `*mut c_void` to match the C++
  typedef. The detour backend in 2d will need a real handle; that call
  will go through the `windows` crate.

**Not yet ported / missing:**
- Every Windows-specific function body referenced above. The 2c-2g
  sub-batches will replace the stubs.
- No `windows` crate bindings have been pulled in yet. Sub-batch 2c
  will add the necessary features to `Cargo.toml` for
  `LoadLibraryW` / `GetProcAddress`.

### Batch 02a - Platform: Encoding, Numbers, Logging

**Source files ported:** `OSTPlatform/Encoding.cpp`, `OSTPlatform/Numbers.cpp`,
`OSTPlatform/Log.cpp`.

**Added:**
- `src/platform/logging.rs` - the log facade. `Sink` is
  `Arc<dyn Fn(Level, &Source, &str) + Send + Sync>`, stored behind an
  `RwLock<Option<_>>`. Five macros (`ostp_trace!`, `ostp_debug!`,
  `ostp_info!`, `ostp_warn!`, `ostp_error!`) mirror the C++ `OSTP_LOG_*`
  family and capture `file!()` / `line!()` at the call site.
- `src/platform/encoding.rs` - wide <-> UTF-8 conversion using Rust's
  built-in UTF-16 handling. The C++ relies on `WideCharToMultiByte` /
  `MultiByteToWideChar` with `CP_UTF8` and default flags, which
  substitute U+FFFD for invalid input; `String::from_utf16_lossy` and
  `str::encode_utf16` produce identical results without the FFI hop.
- `src/platform/numbers.rs` - the full strict-parser family:
  `parse_u64`, `parse_u32`, `parse_i64`, `parse_i32`,
  `parse_hex_u64`, `parse_hex_u32`, `parse_hex_u8`, each with
  `_slice` (byte-range) and `_wide` (UTF-16) variants.
- `src/platform/mod.rs` - module declarations for the three.

**Verified:**
- `wide_to_utf8` / `utf8_to_wide` round-trip ASCII, BMP, and non-BMP
  text. Lone surrogates become U+FFFD (matches `WideCharToMultiByte`).
- All parsers reject empty input, leading `+`, whitespace, and overflow.
  `parse_i64("-0")` returns `Some(0)`, matching `from_chars`.
- Hex variants strip an optional `0x` / `0X` prefix. `parse_hex_u64("0x")`
  fails after stripping to an empty string.
- Slice variants handle out-of-range offsets and cap overlong counts
  (`apply_slice("abcdef", TextSlice::new(4, 100))` -> `"ef"`).
- Log macros write the caller's `file!()` / `line!()` into the `Source`.

**Not yet ported / missing:**
- Batch 2b (`include/` headers) - the trait surface for the Windows
  backends (`DirectoryWatch`, `DynamicLibrary`, `Memory`, `PE`,
  `Process`, `RemoteProcess`, `SteamCredentialStore`, `Trap`, `Detour`,
  `Dialog`, `Hash`, `Http`, `ByteSearch`, `Stopwatch`, `Thread`).
- All Windows backends (sub-batches 2c-2g).

### Batch 01.2 - NetPkt API + warning suppression

**Changed:**
- `src/steam/net_packet.rs` - replaced the reference-returning
  `data_ptr` / `size_ptr` API with four functions: `read_data`,
  `write_data`, `read_size`, `write_size`. The C++ header returned
  `uint8*&` / `uint32&` (references to the pointer/u32 fields), which
  the call sites use exclusively as load/store pairs. A Rust `&mut *mut u8`
  forces an alignment invariant that the packet does not guarantee -
  the previous tests panicked with `misaligned pointer dereference`.
  The new API uses `read_unaligned` / `write_unaligned` and accepts
  packets at any alignment.
- Removed `#[no_mangle]` from the `detail` scratch statics. They are
  private to the crate and do not need external linkage.

**Fixed:**
- `src/steam/net_packet.rs` tests now use byte-aligned stack buffers to
  exercise the unaligned paths deliberately.

**Added:**
- `#![allow(non_snake_case)]` at the crate root. The C++ field names
  (`m_pMemory`, `ManifestGid`, `m_eResult`, ...) are preserved
  deliberately for layout clarity; the 122 warnings this produced are
  now suppressed. Renaming them would obscure the correspondence with
  the source headers.

### Batch 01.1 - Layout fixes + proto

**Fixed:**
- `src/steam/structs.rs` — `KeyValuesValue` union now derives `Copy` and
  `Clone` so `KeyValues` can too. Rust allows this when every union field
  is `Copy`; all fields here are raw pointers, integers, or fixed-size
  arrays.
- `src/steam/ipc_messages.rs` — the packed-struct test now reads fields
  via `core::ptr::read_unaligned` from field addresses instead of field
  access (which would create a misaligned reference). Same change would
  apply to any runtime access to `ExtendedMsgHdr`, `MsgClientGetLegacyGameKey`,
  or `MsgClientGetLegacyGameKeyResponse`.

**Added:**
- `proto/steam_messages.proto` — hand-maintained copy of the protobuf
  schemas the tool reads or writes. Inert for now; will be wired up via
  `prost-build` in the build batch. Field numbers and wire types must
  stay in lockstep with Valve's actual messages.

### Batch 01 - Steam foundation types

**Source files ported:** `Steam/Types.h`, `Steam/Enums.h`, `Steam/Structs.h`,
`Steam/NetPacket.h`, `Steam/Callback.h`, `Steam/IPCMessages.steamd`.

**Added:**
- `src/steam/types.rs` - primitive aliases and ID constants. Wrapped
  values (`k_uPackageIdWallet` = `0xFFFFFFFE`, etc.) match the C++
  assignment of `-2`/`-3` to a `uint32_t`.
- `src/steam/enums.rs` - all small enums, the full `EResult` (~130
  variants), and `EMsg` as a `#[repr(transparent)]` newtype because the
  C++ enum has aliased values that a Rust enum cannot represent. Every
  name and alias is preserved as an associated const.
- `src/steam/structs.rs` - `CUtlMemory<T>`, `CUtlVector<T>`, `CUtlBuffer`,
  `PackageInfo`, `AppOwnership`, `DepotEntry`, `KeyValues`,
  `IKeyValuesSystem` (vtable modelled explicitly), `CNetPacket` (opaque),
  `MsgHdr`, `ExtendedMsgHdr`, `MsgClientGetLegacyGameKey`,
  `CPipeClient`, `CGameID`, `CSteamID`, `CAppData`, `CSteamApp`.
  Bitfield containers (`KeyValues::m_iPackedKeyAndType`,
  `CSteamID`, `CGameID`) are plain integers with explicit mask/shift
  accessors.
- `src/steam/net_packet.rs` - the runtime layout probe. Preserves the
  single-word atomic state machine and the trash-slot fallback.
- `src/steam/callback.rs` - `EncryptedAppTicketResponse_t` and
  `AppLicensesChanged_t` with layout tests matching the C++ offsets.
- `src/steam/ipc_messages.rs` - the IPC enums, transport envelopes
  (packed), handshake struct, and byte-slice facades for
  `IPCRequest` / `IPCInterfaceCall`.
- Inline `#[cfg(test)] mod tests` in every file.

**Verified:**
- All struct sizes match their C++ `static_assert` declarations.
- All documented field offsets in `AppLicensesChanged_t` match.
- `EMsg` alias values equal their base names.
- `CSteamID` round-trips through `u64`.
- `NetPkt` state transitions: unresolved -> latched -> resolved, and
  unresolved -> disabled -> terminal.

**Not yet ported / missing:**
- `proto/steam_messages.proto` is required for the protobuf-generated
  types (`CMsgProtoBufHeader`, `CMsgClientPICSProductInfoRequest`,
  `CContentServerDirectory_GetManifestRequestCode_*`, etc.). This file
  was not in the batch. Needed before Batch 06 (hooks) and Batch 09
  (build).
- `CPipeClient::DebugString` (C++) replaced by an explicit `Debug`
  impl only where useful - callers now format the fields they need.
- `CUtlBuffer`'s member-function pointers are modelled as free-function
  pointers of the same size. If any call site needs to invoke them we
  will need to switch to a `this`-aware representation.

### Added - porting progress
- Initial Rust project scaffold (`rusty_tools` crate).
- Module tree mirroring the C++ layout: `utils`, `steam`, `platform`,
  `pipe`, `hook`, plus a Windows `dllmain` stub.
- `Cargo.toml` with the full dependency set for the port
  (tokio, reqwest, serde, tracing, mlua, prost, windows, retour).
- `scripts\compile.bat` for use inside a Visual Studio developer console.
- `CHANGELOG.md` and `README.md`.

### Pending - next batches
- [ ] Batch 1 - foundation types (`Steam/Types.h`, `Enums.h`, `Structs.h`,
      `NetPacket.h`, `Callback.h`), `proto/steam_messages.proto`,
      `Steam/IPCMessages.steamd`.
- [ ] Batch 2 - `OSTPlatform/` (Win32 wrappers).
- [ ] Batch 3 - `Utils/Logging`, `Utils/Support`, `Utils/Config`.
- [ ] Batch 4 - `Utils/SteamMetadata`.
- [ ] Batch 5 - `Utils/Tickets`, `Utils/CloudRedirect`, `Utils/Tokeer`.
- [ ] Batch 6 - `Hook/` (hooking layer).
- [ ] Batch 7 - `Pipe/`.
- [ ] Batch 8 - `dllmain.cpp`, `dwmapi/`, `xinput1_4/`.
- [ ] Batch 9 - `build.rs` equivalent of the CMake codegen flow.

---

## Conventions used during porting

- **Line-for-line fidelity where it matters.** Struct layouts, protobuf
  tags, IPC envelope fields, and hook calling conventions are preserved
  exactly. Everything else is translated idiomatically.
- **`unsafe` is scoped and documented.** Raw pointer arithmetic, FFI
  boundaries, and Win32 calls live in small `unsafe` blocks with a comment
  explaining the invariant.
- **One module per patch.** Each batch lands on its own, compiles clean,
  and is committed before the next begins.
- **`cargo check` after every file.** Translation errors surface early
  instead of accumulating.
