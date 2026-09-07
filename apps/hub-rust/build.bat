@echo off
setlocal enabledelayedexpansion

echo ==================================================
echo   Building Meridian Hub (Pure Rust - Windows)
echo ==================================================

cd /d "%~dp0"

where cargo >nul 2>nul
if %errorlevel% neq 0 (
    echo [ERROR] Cargo / Rust not found in PATH!
    echo Please install Rust from https://rustup.rs
    pause
    exit /b 1
)

cargo build --release

if not exist "dist" mkdir dist
copy /y "target\release\meridian-hub.exe" "dist\meridian.exe"

if exist "dist\meridian.exe" (
    echo ==================================================
    echo   SUCCESS! Standalone binary created:
    echo   dist\meridian.exe
    echo ==================================================
) else (
    echo [ERROR] Build failed -- meridian.exe not found in target\release
)

pause
