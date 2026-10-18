<p align="center">
  <img src="https://raw.githubusercontent.com/Asper0000/rusty-tools/bced465d965c713ff4f68138787eae40798be40a/src/public/icon-animated.svg" width="160" height="160" alt="RustyTools logo">
</p>

<h1 align="center">RustyTools</h1>

<p align="center">
  <strong>Unlock nearly every game on Steam, and all of its DLCs.</strong><br>
  A free, open-source Steam unlocker. Fork of <a href="https://github.com/madoiscool/BetterSteamTools">BetterSteamTools</a>, rewritten from the ground up in Rust.
</p>

<p align="center">
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.75%2B-orange?logo=rust&logoColor=white" alt="Rust"></a>
  <a href="#"><img src="https://img.shields.io/badge/Platform-Windows%2010%2F11%20x64-0078D6?logo=windows&logoColor=white" alt="Windows"></a>
</p>

<p align="center">
  <strong>English</strong> · <a href="README.zh-CN.md">简体中文</a>
</p>

---

## Contents

- [What is RustyTools?](#what-is-rustytools)
- [Features](#features)
- [How it works](#how-it-works)
- [What doesn't work: Denuvo](#what-doesnt-work-denuvo)
- [Installation](#installation)
- [Configuration](#configuration)
- [Command-line tools](#command-line-tools)
- [Technical details](#technical-details)
- [Educational disclaimer](#educational-disclaimer)
- [License](#license)
- [Credits](#credits)

---

## What is RustyTools?

**RustyTools** is a lightweight add-on for the Steam client that makes Steam treat your account as if it owns the games and DLCs you tell it to. Add an appid to a small Lua script, restart Steam, and the game shows up in your library, ready to install and play.

Under the hood, RustyTools loads into the Steam client at startup and quietly answers Steam's own "do I own this?" questions the way you'd want. Steam never knows the difference, and the games install and launch normally.

It's a fork of the C++ project **[BetterSteamTools](https://github.com/madoiscool/BetterSteamTools)**, fully rewritten in Rust for better memory safety, cleaner concurrency, and a modern toolchain. Everything the original could do, RustyTools does, plus a few extras.

- **Free and open-source.** No accounts, no tokens, no premium tiers.
- **No install.** Drop three DLLs next to `steam.exe` and restart Steam.
- **Reversible.** Delete the DLLs and Steam returns to normal.
- **Config lives in plain text.** TOML for settings, Lua for game lists.

---

## Features

- 🔓 **Unlock any game on Steam**: every store page, every region, every platform
- 🎁 **All DLCs unlocked automatically**: including deluxe editions, season passes, and cosmetic packs
- **Achievements and stats**: works with the achievement system
- **Automatic depot decryption**: no manual key entry needed
- **Manifest pre-caching**: games start downloading instantly
- **`bst://` link handler**: one-click redemption from browsers
- **Hot-reload config**: add or remove games without restarting Steam
- **Lua script support**: flexible per-game tweaks and overrides
- 🛡️ **Safe to remove**: no permanent changes to your Steam install

---

## How it works

RustyTools loads into `steam.exe` the moment Steam starts. From there it hooks a small number of functions inside Steam's own code and answers them the way you'd want:

| Steam asks... | RustyTools answers... |
|---|---|
| "Does this user own app 730?" | Yes |
| "Which depots can they install?" | All of them |
| "What's the decryption key for this depot?" | Here you go |
| "Should this game appear in the library?" | Absolutely |

The Steam client is very literal: if it's told you own a game, it behaves exactly as if you did. The install button appears, the download starts, the game launches, and the anti-cheat handshake works, because from Steam's perspective, none of it was a lie.

**Nothing is downloaded from Valve's servers that isn't already public.** RustyTools never touches your payment details, your account credentials, or your friends list. It only changes what Steam *thinks* about game ownership, and only for the games you list in your config.

---

## What doesn't work: Denuvo

Some recent AAA releases are protected with **Denuvo**, a DRM system that requires a fresh, cryptographically signed **entitlement ticket** (an "e-ticket") to launch. Steam itself doesn't issue these. They come from a server that has a real owning account behind it, and they expire.

To make Denuvo-protected games launchable, a tool would have to run **a server that owns the game, stays online 24/7, and mints a fresh e-ticket every time you press Play**. That means:

- A **real paying account** for every protected game
- **Always-on virtual machines** to keep those accounts logged in
- Bandwidth and uptime costs that scale with every user

We can't afford to run that, and we wouldn't want to, since it's the exact kind of thing that would make RustyTools unsustainable and legally messy. **So Denuvo-protected games are not supported.** If a game is protected by Denuvo, it will install and update fine, but it will not launch past the DRM check.

Games **without** Denuvo, which is the overwhelming majority of Steam's catalogue (including the vast majority of indies, older AAA titles, and mid-budget releases), work perfectly.

> **Tip:** Check a game's store page or [PCGamingWiki](https://www.pcgamingwiki.com/) before trying it. If it lists "Denuvo Anti-Tamper," it won't launch.

---

## Installation

### Requirements

- **Windows 10 or 11** (64-bit)
- **Steam** installed and logged in at least once
- No other Steam-modifying tools running

### Steps

1. **Download** the latest release from the [Releases page](../../releases).
2. **Exit Steam completely** (tray icon -> *Exit*, not just close the window).
3. **Copy three files** into your Steam install folder (usually `C:\Program Files (x86)\Steam\`):
   - `RustyTools.dll`
   - `dwmapi.dll`
   - `xinput1_4.dll`
4. **Start Steam.**
5. **Add games** to `rustytools.toml`, see [Configuration](#configuration) below.
6. **Restart Steam** once, and the games appear in your library.

### Uninstall

Delete the three DLLs from the Steam folder and restart Steam. Everything returns to normal: no registry keys, no leftover files, no traces.

---

## Configuration

All game selection lives in **`rustytools.toml`** (next to `steam.exe`) and in **`*.lua` scripts** under `Steam\config\stplug-in\`.

### `rustytools.toml`

A TOML file that configures RustyTools itself: logging, manifest provider, optional Cloud redirect, and so on. It's optional; sensible defaults apply if it's missing. The full example with inline documentation is in [`config/rustytools.example.toml`](config/rustytools.example.toml).

### Lua scripts: your game list

This is where you tell RustyTools which games to unlock. Create `Steam\config\stplug-in\my-games.lua`:

```lua
-- Unlock Portal 2 (appid 620)
addappid(620)

-- Unlock all DLCs
addappid(620, 0, "DLC_DEPOT_ID_HERE")

-- Unlock Half-Life: Alyx
addappid(546560)

-- Force a specific decryption key for a depot
addappid(546561, 0, "0123456789abcdef0123456789abcdef")
```

Changes are picked up **while Steam is running**. No restart needed. Add a game, and it appears in the library within a second or two.

### Where do I find appids?

- **The game's store page URL:** `store.steampowered.com/app/**620**/Portal_2/`
- **SteamDB:** search the game name, appid is at the top
- **Community lists:** many are pinned in the RustyTools discussions

### Removing a game

Delete the `addappid(...)` line from the Lua script and save. Steam hides the game from the library within a few seconds. It stays hidden across restarts, cache clears, and fresh logins.

---

## Command-line tools

RustyTools ships with `ost-cli.exe`, a small developer tool for testing configs and running handy utilities outside Steam.

```text
ost-cli version                      Print the library version
ost-cli init [--config PATH]         Load and print a TOML config
ost-cli parse-lua PATH               Parse a Lua script and print the depot set
ost-cli parse-toml PATH              Print the effective TOML settings
ost-cli sha256 PATH                  SHA-256 of a file (for pattern cache keys)
ost-cli fnv1a STRING                 FNV-1a hash (used internally for signatures)
ost-cli handle-uri URL               Simulate a bst:// link without a browser
ost-cli register-uri PATH            Register the bst:// URL scheme once
```

---

## Technical details

*Skip this section if you just want to play games.*

### Architecture

RustyTools is a native Windows DLL that loads into `steam.exe` via **loader hijacking**. Two proxy DLLs (`dwmapi.dll` and `xinput1_4.dll`) are placed alongside `steam.exe` and loaded in preference to the system copies (Windows searches the application directory before `System32`). Each proxy forwards its entire export table to the real system DLL, and only loads `RustyTools.dll` when the host process is `steam.exe`.

Once loaded, `RustyTools.dll` spawns an init thread that:

1. Resolves Steam's internal functions via **byte-pattern scanning** (signatures are fetched per Steam build; fallbacks are cached locally)
2. Parses your TOML config and Lua scripts
3. Installs **Detours hooks** on roughly a dozen Steam functions
4. Starts file watchers for live config reload
5. Enters idle mode

The hook layer covers:

| Layer | What it does |
|---|---|
| **IPC** | Intercepts Steam's IPC messages between the client and `steamclient64.dll` |
| **NetPacket** | Rewrites WebSocket protobuf packets (licenses, stats, presence, manifests) |
| **Package** | Injects a fake license package that grants ownership of your configured appids |
| **Manifest** | Pre-seeds and pre-fetches depot manifests so games start downloading immediately |
| **Decryption** | Serves AES keys for depots; keys come from your Lua config or Steam's own stores |
| **SteamUI** | Keeps removed games hidden across full library rebuilds |
| **Misc** | Handles `-onlinefix` launch options and appid spoofing where needed |

### Build

Requirements:

- Windows 10/11 x64
- Visual Studio 2022 (MSVC x64 toolchain)
- Rust 1.75+
- [Microsoft Detours](https://github.com/microsoft/Detours) static library, vendored into `third_party/Detours/`

From an x64 Native Tools Command Prompt:

```bat
git clone https://github.com/microsoft/Detours.git third_party\Detours
cd third_party\Detours\src
set DETOURS_TARGET_PROCESSOR=X64
nmake
cd ..\..\..
cargo build --workspace --release
```

Output lands in `target\release\`:

- `RustyTools.dll`, the injected hook DLL
- `dwmapi.dll`, `xinput1_4.dll`: loader-hijack proxies
- `ost-cli.exe`, developer CLI

See [`docs/TESTING.md`](docs/TESTING.md) for the manual verification checklist and [`docs/STUBS.md`](docs/STUBS.md) for the (short) list of documented divergences.

### How patterns are fetched

Steam updates its client frequently. Hook targets are resolved by **signature** (byte patterns) rather than fixed offsets, and the pattern files are regenerated for each Steam build. If a pattern file is missing or stale, RustyTools falls back to a locally cached signature set; if a function can't be found at all, only that specific hook is disabled. Steam still starts normally.

### Data layout

| File / folder | Purpose |
|---|---|
| `rustytools.toml` | Main configuration (next to `steam.exe`) |
| `config\stplug-in\*.lua` | Per-game unlock lists |
| `rustytools\logs\*.log` | One log per module (`main`, `ipc`, `netpacket`, ...) |
| `rustytools\pattern\` | Cached signature files keyed by DLL SHA-256 |
| `rustytools\ipc\` | Cached IPC-method spec keyed by DLL SHA-256 |

---

## Educational disclaimer

**RustyTools is provided for educational and research purposes only.**

This project exists to study how Windows process injection, function hooking, Steam's IPC protocol, depot manifests, and DRM systems work at the systems level. It is not a product and is not distributed as one.

**Using RustyTools to interact with Steam may violate the [Steam Subscriber Agreement](https://store.steampowered.com/subscriber_agreement/) and could result in your account being restricted or terminated.** Using RustyTools to obtain access to content you have not paid for may violate copyright law in your jurisdiction.

The authors and contributors of RustyTools:

- **Do not condone** using this software to avoid paying for content you would otherwise purchase
- **Do not provide support** for any use that violates a game's EULA, Steam's Terms of Service, or any applicable law
- **Accept no liability** for any consequences, including account bans, legal action, corrupted saves, lost data, or anything else, that arise from using this software

**Any use of RustyTools is entirely at your own risk, and entirely on you.**

If you're not studying the code for its own sake, this project is not for you.

---

## License

RustyTools is licensed under the **GNU General Public License v3.0** (GPLv3).

This project is a **fork of [BetterSteamTools](https://github.com/madoiscool/BetterSteamTools)**, rewritten in Rust and substantially modified. It contains code and design work derived from the original project. The original authorship is acknowledged and preserved under the terms of its license; the combined work here is distributed under GPLv3 to comply with those terms.

The full license text is in [`LICENSE`](LICENSE). By using, modifying, or redistributing this project, you agree to the terms of the GPLv3:

- You may **use** RustyTools for any purpose
- You may **study and modify** the source
- You may **redistribute** modified versions **only under the same GPLv3 license** and **only with source code available**
- You may **not** add restrictions that limit others' freedoms under GPLv3

---

## Credits

- **[BetterSteamTools](https://github.com/madoiscool/BetterSteamTools)**: the original C++ project this is forked from
- **[OpenSteamTool](https://github.com/madoiscool/OpenSteamTool)**: the spiritual ancestor of the whole ecosystem
- **[Microsoft Detours](https://github.com/microsoft/Detours)**: the function-hooking library everything is built on
- **[mlua](https://github.com/mlua-rs/mlua)** and **[prost](https://github.com/tokio-rs/prost)**: Rust's Lua and protobuf crates
- **The Steam modding community**: for years of reverse-engineering that made all of this possible

---

<p align="center">
  <em>If RustyTools makes your day better, consider starring the repo ⭐</em>
</p>