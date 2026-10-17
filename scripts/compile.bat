
@echo off
setlocal
cd /d "%~dp0.."

echo ============================================
echo  Compile - rusty_tools
echo ============================================
echo.

if not defined VSCMD_VER goto :no_dev_env
echo [ok] VS Developer environment: %VSCMD_VER%
echo.

if not exist "Cargo.toml" goto :no_cargo

set "ACTION=%~1"
if "%ACTION%"=="" set "ACTION=build"

set "CARGO_CMD="
if /i "%ACTION%"=="build"   set "CARGO_CMD=cargo build"
if /i "%ACTION%"=="release" set "CARGO_CMD=cargo build --release"
if /i "%ACTION%"=="check"   set "CARGO_CMD=cargo check"
if /i "%ACTION%"=="test"    set "CARGO_CMD=cargo test"
if /i "%ACTION%"=="clean"   set "CARGO_CMD=cargo clean"
if /i "%ACTION%"=="fmt"     set "CARGO_CMD=cargo fmt"
if /i "%ACTION%"=="clippy"  set "CARGO_CMD=cargo clippy --all-targets -- -D warnings"
if /i "%ACTION%"=="run"     set "CARGO_CMD=cargo run"

if not defined CARGO_CMD goto :bad_action

echo [..] %CARGO_CMD%
echo.
%CARGO_CMD%
if errorlevel 1 goto :failed

echo.
echo [ok] Success.
if /i "%ACTION%"=="build"   echo Binary: target\debug\ost-cli.exe
if /i "%ACTION%"=="build"   echo DLLs  : target\debug\RustyTools.dll, dwmapi.dll, xinput1_4.dll
if /i "%ACTION%"=="release" echo Binary: target\release\ost-cli.exe
if /i "%ACTION%"=="release" echo DLLs  : target\release\RustyTools.dll, dwmapi.dll, xinput1_4.dll
if /i "%ACTION%"=="run"     echo Binary: target\debug\ost-cli.exe
echo.
pause
exit /b 0

:no_dev_env
echo [!!] This script must be run from a Visual Studio Developer Command Prompt.
echo.
echo      From the Start menu, open:
echo        "x64 Native Tools Command Prompt for VS 2022"
echo        (or whatever version matches your install^)
echo.
echo      Then:
echo        cd "%CD%"
echo        compile.bat %*
echo.
pause
exit /b 1

:no_cargo
echo [!!] Cargo.toml not found in %CD%
pause
exit /b 1

:bad_action
echo [!!] Unknown action: %ACTION%
echo.
echo      Usage: compile.bat [action]
echo.
echo        (none)    debug build
echo        release   release build
echo        check     fast type-check
echo        test      run tests
echo        clean     clean target
echo        fmt       format code
echo        clippy    clippy with warnings-as-errors
echo        run       run the CLI binary
echo.
pause
exit /b 1

:failed
echo.
echo [!!] Compilation failed. See output above.
echo.
pause
exit /b 1
