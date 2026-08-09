@echo off
setlocal

title RustCraft Launcher
cd /d "%~dp0"
if errorlevel 1 (
    echo [RustCraft] Failed to enter the project directory.
    pause
    exit /b 1
)

where cargo >nul 2>&1
if errorlevel 1 (
    echo [RustCraft] Cargo was not found in PATH.
    echo Install Rust from https://rustup.rs/ and reopen this script.
    pause
    exit /b 1
)

if not defined CARGO_TARGET_DIR set "CARGO_TARGET_DIR=%~dp0target"

echo [RustCraft] Building and launching...
cargo run -p rustcraft-game -- %*
set "EXIT_CODE=%ERRORLEVEL%"

if not "%EXIT_CODE%"=="0" (
    echo.
    echo [RustCraft] Launch failed with exit code %EXIT_CODE%.
    pause
)

exit /b %EXIT_CODE%
