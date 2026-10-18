@echo off
setlocal
cd /d "%~dp0"
if not exist "Cargo.toml" ( cd /d "%~dp0.." )

echo ============================================
echo  rusty_tools: DllMain logging + cleanup
echo ============================================
echo.

if not exist "Cargo.toml" (
    echo [!!] run from the repo root
    goto :failed
)

set "PSFILE=%TEMP%\ost_dllmain_fix.ps1"
powershell -NoProfile -Command "$c = Get-Content -LiteralPath '%~f0' -Raw; $s = $c.LastIndexOf('###PS_START###'); $e = $c.LastIndexOf('###PS_END###'); if ($s -lt 0 -or $e -lt 0) { exit 2 }; $b = $c.Substring($s + 14, $e - $s - 14); [IO.File]::WriteAllText('%PSFILE%', $b, (New-Object System.Text.UTF8Encoding($false))); exit 0"
if errorlevel 1 goto :extract_failed

powershell -NoProfile -ExecutionPolicy Bypass -File "%PSFILE%"
set "RC=%ERRORLEVEL%"
del "%PSFILE%" 2>nul
if not "%RC%"=="0" goto :failed

echo.
echo [ok] patch applied. Next: cargo build
echo.
pause
exit /b 0

:extract_failed
echo [!!] could not extract payload.
pause
exit /b 1

:failed
echo [!!] patch failed.
pause
exit /b 1

REM === do not edit below this line ===
exit /b 0

###PS_START###
$ErrorActionPreference = 'Stop'
$nl    = [Environment]::NewLine
$utf8  = New-Object System.Text.UTF8Encoding($false)

function Read-Src([string]$p) {
    $path = (Resolve-Path -LiteralPath $p).Path
    $c = [IO.File]::ReadAllText($path, $utf8)
    return @{ Path = $path; Text = ($c -replace "`r?`n", $nl) }
}
function Write-Src($src) {
    [IO.File]::WriteAllText($src.Path, $src.Text, $utf8)
}

# ═════════════════════════════════════════════════════════════════════
# Fix 1 — #17: DllMain log calls under the loader lock
# ═════════════════════════════════════════════════════════════════════
$src = Read-Src 'src\dllmain.rs'

# Insert helper after the Win32 reason constants.
$const_anchor = 'const DLL_PROCESS_DETACH: u32 = 0;'
$helper = @'

// ── Loader-lock-safe debug output ───────────────────────────────────
//
// DllMain runs under the Windows loader lock. Calling LOG_INFO! from
// here takes the per-file log mutex and performs disk I/O, which can
// deadlock with a thread that is blocked on the same mutex while
// Detours later tries to suspend it (see #17). OutputDebugStringA is
// safe in a loader-locked context: no file I/O, no lock.
#[cfg(windows)]
fn dllmain_debug(msg: &str) {
    if let Ok(cstr) = std::ffi::CString::new(msg) {
        unsafe {
            windows_sys::Win32::System::Diagnostics::Debug::OutputDebugStringA(
                cstr.as_ptr() as *const u8,
            );
        }
    }
}

#[cfg(not(windows))]
fn dllmain_debug(_msg: &str) {}
'@ -replace "`r?`n", $nl

if (-not $src.Text.Contains('fn dllmain_debug')) {
    if (-not $src.Text.Contains($const_anchor)) {
        Write-Host "[!!] dllmain.rs: DLL_PROCESS_DETACH const not found"
        exit 1
    }
    $src.Text = $src.Text.Replace($const_anchor, $const_anchor + $nl + $helper)
}

# Replace ATTACH's LOG_INFO! (and reuse is_steam_host() for the check).
$attach_old = @'
        DLL_PROCESS_ATTACH => {
            LOG_INFO!("[wf] DllMain DLL_PROCESS_ATTACH is_steam_host={}", is_steam_host());
            windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls(hmodule);
            if !is_steam_host() {
'@ -replace "`r?`n", $nl

$attach_new = @'
        DLL_PROCESS_ATTACH => {
            // Do NOT use LOG_INFO! here. DllMain runs under the loader
            // lock; taking the file mutex can deadlock with Detours'
            // suspend-all during unhook. OutputDebugStringA is safe.
            let is_host = is_steam_host();
            dllmain_debug(if is_host {
                "[wf] DllMain DLL_PROCESS_ATTACH is_steam_host=true"
            } else {
                "[wf] DllMain DLL_PROCESS_ATTACH is_steam_host=false"
            });
            windows_sys::Win32::System::LibraryLoader::DisableThreadLibraryCalls(hmodule);
            if !is_host {
'@ -replace "`r?`n", $nl

if ($src.Text.Contains($attach_old)) {
    $src.Text = $src.Text.Replace($attach_old, $attach_new)
} elseif ($src.Text.Contains('dllmain_debug(if is_host')) {
    Write-Host "[=] dllmain.rs: ATTACH already patched"
} else {
    Write-Host "[!!] dllmain.rs: ATTACH anchor not found"
    exit 1
}

# Replace DETACH's LOG_INFO!.
$detach_old = '            LOG_INFO!("[wf] DllMain DLL_PROCESS_DETACH");'
$detach_new = '            // Loader-lock-safe: see comment in the ATTACH branch.' + $nl +
              '            dllmain_debug("[wf] DllMain DLL_PROCESS_DETACH");'

if ($src.Text.Contains($detach_old)) {
    $src.Text = $src.Text.Replace($detach_old, $detach_new)
} elseif ($src.Text.Contains('dllmain_debug("[wf] DllMain DLL_PROCESS_DETACH")')) {
    Write-Host "[=] dllmain.rs: DETACH already patched"
} else {
    Write-Host "[!!] dllmain.rs: DETACH anchor not found"
    exit 1
}

Write-Src $src
Write-Host "[ok] dllmain.rs: DllMain logging replaced with OutputDebugStringA"


# ═════════════════════════════════════════════════════════════════════
# Fix 2 — #13: decode_hex no longer allocates a String per byte pair
# ═════════════════════════════════════════════════════════════════════
$src = Read-Src 'src\utils\config\lua_config.rs'

$decode_old = @'
fn decode_hex(hex: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity((hex.len() + 1) / 2);
    let bytes = hex.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let a = bytes[i] as char;
        let b = if i + 1 < bytes.len() { bytes[i + 1] as char } else { '0' };
        let s: String = [a, b].iter().collect();
        out.push(parse_hex_byte(&s));
        i += 2;
    }
    out
}
'@ -replace "`r?`n", $nl

$decode_new = @'
fn decode_hex(hex: &str) -> Vec<u8> {
    // Called once per config load from setappticket / seteticket, so the
    // old String-per-pair allocation was harmless in practice. This
    // version allocates only the output Vec and preserves the previous
    // behaviour exactly, including for malformed input (any non-hex
    // nibble in a pair yields 0).
    fn nibble(c: u8) -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity((bytes.len() + 1) / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = nibble(bytes[i]);
        let lo = if i + 1 < bytes.len() { nibble(bytes[i + 1]) } else { Some(0) };
        match (hi, lo) {
            (Some(h), Some(l)) => out.push((h << 4) | l),
            _ => out.push(0),
        }
        i += 2;
    }
    out
}
'@ -replace "`r?`n", $nl

if ($src.Text.Contains($decode_old)) {
    $src.Text = $src.Text.Replace($decode_old, $decode_new)
    Write-Host "[ok] lua_config.rs: decode_hex is now zero-allocation"
} elseif ($src.Text.Contains('fn nibble(c: u8) -> Option<u8>')) {
    Write-Host "[=] lua_config.rs: decode_hex already patched"
} else {
    Write-Host "[!!] lua_config.rs: decode_hex anchor not found"
    exit 1
}

Write-Src $src


# ═════════════════════════════════════════════════════════════════════
# Fix 3 — stale doc comments in bridge.rs on the ticket-write stubs
# ═════════════════════════════════════════════════════════════════════
$src = Read-Src 'src\bridge.rs'

$old1 = '/// Delegates to `utils::tickets::app_ticket::write_app_ownership_ticket`.' + $nl +
        'pub fn appticket_write_ownership_ticket(app_id: u32, _data: &[u8]) -> bool {'
$new1 = '/// Stub: the Denuvo ticket pipeline was removed from this build (see' + $nl +
        '/// `src/unused_denuvo/README.md`). Returns `true` so Lua `setappticket`' + $nl +
        '/// calls do not raise; nothing is written to the credential store.' + $nl +
        'pub fn appticket_write_ownership_ticket(app_id: u32, _data: &[u8]) -> bool {'

$old2 = '/// Delegates to `utils::tickets::app_ticket::write_encrypted_ticket`.' + $nl +
        'pub fn appticket_write_encrypted_ticket(app_id: u32, _data: &[u8]) -> bool {'
$new2 = '/// Stub: see `appticket_write_ownership_ticket` above.' + $nl +
        'pub fn appticket_write_encrypted_ticket(app_id: u32, _data: &[u8]) -> bool {'

$changed = $false
if ($src.Text.Contains($old1)) { $src.Text = $src.Text.Replace($old1, $new1); $changed = $true }
if ($src.Text.Contains($old2)) { $src.Text = $src.Text.Replace($old2, $new2); $changed = $true }

if ($changed) {
    Write-Src $src
    Write-Host "[ok] bridge.rs: stale delegation comments corrected"
} else {
    Write-Host "[=] bridge.rs: comments already correct"
}


# ═════════════════════════════════════════════════════════════════════
# Fix 4 — #14: remove #![allow(unused_imports)] (partial)
# ═════════════════════════════════════════════════════════════════════
$src = Read-Src 'src\lib.rs'

$allow_old = '#![allow(dead_code)]' + $nl + '#![allow(unused_imports)]'
$allow_new = '// NOTE: `dead_code` is still allowed pending cleanup of porting-fidelity' + $nl +
             '// stubs (the C++ tree deliberately keeps some unwired functions).' + $nl +
             '// `unused_imports` is NOT: an unused import almost always means a' + $nl +
             '// refactor forgot to rewire a call site, and none of them are needed' + $nl +
             '// for FFI the way `non_snake_case` / `non_camel_case_types` are.' + $nl +
             '#![allow(dead_code)]'

if ($src.Text.Contains($allow_old)) {
    $src.Text = $src.Text.Replace($allow_old, $allow_new)
    Write-Src $src
    Write-Host "[ok] lib.rs: removed #![allow(unused_imports)]"
} elseif ($src.Text.Contains('`unused_imports` is NOT')) {
    Write-Host "[=] lib.rs: already patched"
} else {
    Write-Host "[!!] lib.rs: allow(unused_imports) anchor not found"
    exit 1
}

Write-Host ""
Write-Host "All patches applied. Run: cargo build"
exit 0
###PS_END###