@echo off
setlocal
cd /d "%~dp0"
if not exist "Cargo.toml" ( cd /d "%~dp0.." )

echo ============================================
echo  rusty_tools: stub appticket bridge
echo ============================================
echo.

if not exist "Cargo.toml" (
    echo [!!] run from the repo root
    goto :failed
)

set "PSFILE=%TEMP%\ost_stub_bridge.ps1"
powershell -NoProfile -Command "$c = Get-Content -LiteralPath '%~f0' -Raw; $s = $c.LastIndexOf('###PS_START###'); $e = $c.LastIndexOf('###PS_END###'); if ($s -lt 0 -or $e -lt 0) { exit 2 }; $b = $c.Substring($s + 14, $e - $s - 14); [IO.File]::WriteAllText('%PSFILE%', $b, (New-Object System.Text.UTF8Encoding($false))); exit 0"
if errorlevel 1 goto :extract_failed

powershell -NoProfile -ExecutionPolicy Bypass -File "%PSFILE%"
set "RC=%ERRORLEVEL%"
del "%PSFILE%" 2>nul
if not "%RC%"=="0" goto :failed

echo.
echo [ok] bridge.rs stubbed successfully.
echo Next: cargo build --release
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
$path = 'src\bridge.rs'
$c = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $path))

# Replace the ownership ticket function
$pattern1 = '(?s)pub fn appticket_write_ownership_ticket\(app_id: u32, data: &\[u8\]\) -> bool \{.*?\}'
$replacement1 = @'
pub fn appticket_write_ownership_ticket(app_id: u32, _data: &[u8]) -> bool {
    LOG_WARN!(
        "appticket_write_ownership_ticket(app={}): Denuvo backend removed from this build; ticket ignored",
        app_id
    );
    true
}
'@
$c = [regex]::Replace($c, $pattern1, $replacement1)

# Replace the encrypted ticket function
$pattern2 = '(?s)pub fn appticket_write_encrypted_ticket\(app_id: u32, data: &\[u8\]\) -> bool \{.*?\}'
$replacement2 = @'
pub fn appticket_write_encrypted_ticket(app_id: u32, _data: &[u8]) -> bool {
    LOG_WARN!(
        "appticket_write_encrypted_ticket(app={}): Denuvo backend removed from this build; ticket ignored",
        app_id
    );
    true
}
'@
$c = [regex]::Replace($c, $pattern2, $replacement2)

[IO.File]::WriteAllText((Resolve-Path -LiteralPath $path), $c)
Write-Host "[ok] patched appticket_write_* in bridge.rs"
exit 0
###PS_END###