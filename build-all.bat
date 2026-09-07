@echo off
cd /d "%~dp0"
echo Building Meridian Hub (Pure Rust - Windows)...
call apps\hub-rust\build.bat %*
