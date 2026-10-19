@echo off
setlocal
cd /d "%~dp0"
if not exist "Cargo.toml" ( cd /d "%~dp0.." )

echo ============================================
echo  rusty_tools: fix build error + warnings
echo ============================================
echo.

if not exist "Cargo.toml" ( echo [!!] run from the repo root & goto :failed )

set "PSFILE=%TEMP%\ost_fix_build.ps1"
powershell -NoProfile -Command "$c = Get-Content -LiteralPath '%~f0' -Raw; $s = $c.LastIndexOf('###PS_START###'); $e = $c.LastIndexOf('###PS_END###'); if ($s -lt 0 -or $e -lt 0) { exit 2 }; $b = $c.Substring($s + 14, $e - $s - 14); [IO.File]::WriteAllText('%PSFILE%', $b, (New-Object System.Text.UTF8Encoding($false))); exit 0"
if errorlevel 1 goto :extract_failed

powershell -NoProfile -ExecutionPolicy Bypass -File "%PSFILE%"
set "RC=%ERRORLEVEL%"
del "%PSFILE%" 2>nul
if not "%RC%"=="0" goto :failed

echo.
echo [ok] patch applied. Next: cargo build --workspace --release
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
    if (-not (Test-Path $p)) { return $null }
    $path = (Resolve-Path -LiteralPath $p).Path
    $c = [IO.File]::ReadAllText($path, $utf8)
    return @{ Path = $path; Text = ($c -replace "`r?`n", $nl) }
}
function Write-Src($src) {
    if ($null -ne $src) {
        [IO.File]::WriteAllText($src.Path, $src.Text, $utf8)
    }
}
function Fail([string]$msg) { Write-Host "[!!] $msg"; exit 1 }

# ─────────────────────────────────────────────────────────────────
# CRITICAL: restore "let mods" in get_module_path
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\platform\remote_process.rs'
if (-not $src) { Fail 'src/platform/remote_process.rs missing' }

$already_fixed = $src.Text -match (
    'let mods = crate::platform::process::enumerate_modules\(pid\);' + $nl +
    '        mods\.into_iter'
)
if ($already_fixed) {
    Write-Host '[skip] get_module_path already restored'
} else {
    $old = '    fn get_module_path(pid: u32, name: &str) -> Option<PathBuf> {' + $nl +
           '        mods.into_iter()'
    $new = '    fn get_module_path(pid: u32, name: &str) -> Option<PathBuf> {' + $nl +
           '        let mods = crate::platform::process::enumerate_modules(pid);' + $nl +
           '        mods.into_iter()'
    if ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        Write-Host '[ok] remote_process.rs: restored let mods in get_module_path'
    } else {
        Fail 'get_module_path pattern not found. Inspect src/platform/remote_process.rs around line 120.'
    }
}

$src.Text = $src.Text.Replace(
    '    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;' + $nl, '')
Write-Src $src
Write-Host '[ok] remote_process.rs: removed unused GetModuleHandleW import'

# ─────────────────────────────────────────────────────────────────
# WARN: pattern_loader.rs - unused self
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\utils\steam_metadata\pattern_loader.rs'
if ($src) {
    $old = 'use crate::platform::dynamic_library::{self, ModuleHandle};'
    $new = 'use crate::platform::dynamic_library::ModuleHandle;'
    if ($src.Text.Contains($new)) {
        Write-Host '[skip] pattern_loader.rs already clean'
    } elseif ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        Write-Src $src
        Write-Host '[ok] pattern_loader.rs: dropped unused self'
    } else {
        Write-Host '[warn] pattern_loader.rs import pattern not found'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: ipc_loader.rs - unused HashSet
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\utils\steam_metadata\ipc_loader.rs'
if ($src) {
    $old = 'use std::collections::{HashMap, HashSet};'
    $new = 'use std::collections::HashMap;'
    if ($src.Text.Contains($new) -and -not $src.Text.Contains($old)) {
        Write-Host '[skip] ipc_loader.rs already clean'
    } elseif ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        Write-Src $src
        Write-Host '[ok] ipc_loader.rs: dropped unused HashSet'
    } else {
        Write-Host '[warn] ipc_loader.rs import pattern not found'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: enums.rs - unused super::types::*
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\steam\enums.rs'
if ($src) {
    $old = 'use super::types::*;' + $nl
    if (-not $src.Text.Contains($old)) {
        Write-Host '[skip] enums.rs already clean'
    } else {
        $src.Text = $src.Text.Replace($old, '')
        Write-Src $src
        Write-Host '[ok] enums.rs: dropped unused super::types::*'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: ipc_messages.rs - unused super::types::*
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\steam\ipc_messages.rs'
if ($src) {
    $old = 'use super::types::*;' + $nl
    if (-not $src.Text.Contains($old)) {
        Write-Host '[skip] ipc_messages.rs already clean'
    } else {
        $src.Text = $src.Text.Replace($old, '')
        Write-Src $src
        Write-Host '[ok] ipc_messages.rs: dropped unused super::types::*'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: encoding.rs - unused Level
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\platform\encoding.rs'
if ($src) {
    $old = 'use crate::platform::logging::Level;' + $nl
    if (-not $src.Text.Contains($old)) {
        Write-Host '[skip] encoding.rs already clean'
    } else {
        $src.Text = $src.Text.Replace($old, '')
        Write-Src $src
        Write-Host '[ok] encoding.rs: dropped unused Level import'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: process.rs - unused OsStrExt
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\platform\process.rs'
if ($src) {
    $old = '    use std::os::windows::ffi::OsStrExt;' + $nl
    if (-not $src.Text.Contains($old)) {
        Write-Host '[skip] process.rs already clean'
    } else {
        $src.Text = $src.Text.Replace($old, '')
        Write-Src $src
        Write-Host '[ok] process.rs: dropped unused OsStrExt'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: steam_credential_store.rs - unused HKEY, REG_SZ, _touch stub
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\platform\steam_credential_store.rs'
if ($src) {
    $changed = $false

    $old = 'use super::windows::handles::{UniqueRegKey, HKEY};'
    $new = 'use super::windows::handles::UniqueRegKey;'
    if ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        $changed = $true
    }

    $old = '        REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,'
    $new = '        REG_OPTION_NON_VOLATILE, RRF_RT_REG_BINARY, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,'
    if ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        $changed = $true
    }

    $old = $nl + '#[cfg(not(windows))]' + $nl + '#[allow(dead_code)]' + $nl + 'fn _touch(_: HKEY) {}' + $nl
    if ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $nl)
        $changed = $true
    }

    if ($changed) {
        Write-Src $src
        Write-Host '[ok] steam_credential_store.rs: dropped unused HKEY, REG_SZ, _touch'
    } else {
        Write-Host '[skip] steam_credential_store.rs already clean'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: pipe_manager.rs - unused PipeKey
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\pipe\pipe_manager.rs'
if ($src) {
    $old = 'use crate::pipe::pipe_types::{make_pipe_key, PipeContext, PipeKey, ProcessKey};'
    $new = 'use crate::pipe::pipe_types::{make_pipe_key, PipeContext, ProcessKey};'
    if ($src.Text.Contains($new) -and -not $src.Text.Contains($old)) {
        Write-Host '[skip] pipe_manager.rs already clean'
    } elseif ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        Write-Src $src
        Write-Host '[ok] pipe_manager.rs: dropped unused PipeKey'
    } else {
        Write-Host '[warn] pipe_manager.rs import pattern not found'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: hook/misc.rs - unused CAppData
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\hook\misc.rs'
if ($src) {
    $old = 'use crate::steam::structs::{CAppData, CGameID, CUtlBuffer};'
    $new = 'use crate::steam::structs::{CGameID, CUtlBuffer};'
    if ($src.Text.Contains($new) -and -not $src.Text.Contains($old)) {
        Write-Host '[skip] hook/misc.rs already clean'
    } elseif ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        Write-Src $src
        Write-Host '[ok] hook/misc.rs: dropped unused CAppData'
    } else {
        Write-Host '[warn] hook/misc.rs import pattern not found'
    }
}

# ─────────────────────────────────────────────────────────────────
# WARN: hook/netpacket/dispatch.rs - unused netpkt + legacy_key
# ─────────────────────────────────────────────────────────────────
$src = Read-Src 'src\hook\netpacket\dispatch.rs'
if ($src) {
    $changed = $false

    $old = 'use crate::steam::net_packet as netpkt;' + $nl
    if ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, '')
        $changed = $true
    }

    $old = '    access_token, family_sharing, legacy_key, licenses, manifest,'
    $new = '    access_token, family_sharing, licenses, manifest,'
    if ($src.Text.Contains($old)) {
        $src.Text = $src.Text.Replace($old, $new)
        $changed = $true
    }

    if ($changed) {
        Write-Src $src
        Write-Host '[ok] dispatch.rs: dropped unused netpkt and legacy_key'
    } else {
        Write-Host '[skip] dispatch.rs already clean'
    }
}

Write-Host ''
Write-Host 'Fixed:'
Write-Host '  * remote_process.rs get_module_path: let mods restored (build error)'
Write-Host '  * All 13 unused-import warnings'
Write-Host ''
Write-Host 'Next: cargo build --workspace --release'
Write-Host '      then cargo clippy --all-targets -- -D warnings if you want a clean signal.'
Write-Host ''
Write-Host 'Heads-up (not fixed, out of scope):'
Write-Host '  steam_credential_store.rs write_value callers pass RRF_RT_REG_* constants'
Write-Host '  as the RegSetValueExW type argument, which should be REG_*. The stored'
Write-Host '  type becomes non-standard and matching RegGetValueW reads would fail.'
Write-Host '  This path is dead today (ticket pipeline moved to unused/), so it is'
Write-Host '  not causing the build failure. Worth a dedicated fix if the ticket'
Write-Host '  writer is ever re-enabled.'
exit 0
###PS_END###