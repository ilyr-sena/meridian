@echo off
setlocal enabledelayedexpansion

echo ==================================================
echo   Meridian Hub -- One-Click Windows Builder
echo ==================================================

cd /d "%~dp0"

:: 1. Locate Python
set PYTHON=
where python >nul 2>nul
if %errorlevel% equ 0 (
    set PYTHON=python
) else (
    where py >nul 2>nul
    if %errorlevel% equ 0 (
        set PYTHON=py -3
    )
)

if "%PYTHON%"=="" (
    echo [ERROR] Python not found in PATH!
    echo Please install Python 3.11 or 3.12 from python.org and ensure "Add to PATH" is checked.
    pause
    exit /b 1
)

echo [1/3] Using Python: %PYTHON%

:: 2. Setup virtual environment if not present
if not exist ".venv\Scripts\python.exe" (
    echo [2/3] Creating virtual environment in .venv...
    %PYTHON% -m venv .venv
)

echo [2/3] Installing / updating build dependencies...
.venv\Scripts\python.exe -m pip install --upgrade pip
.venv\Scripts\python.exe -m pip install -e . pyinstaller srp

:: 3. Run Build
echo [3/3] Compiling standalone meridian.exe...
.venv\Scripts\python.exe packaging\build_binary.py %*

if exist "dist\meridian.exe" (
    echo ==================================================
    echo   SUCCESS! Standalone binary created:
    echo   dist\meridian.exe
    echo ==================================================
) else (
    echo [ERROR] Build failed -- meridian.exe not found in dist\
)

pause
