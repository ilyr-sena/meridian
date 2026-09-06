@echo off
cd /d "%~dp0"
echo Building Meridian Hub (Windows)...
call apps\hub\build.bat %*
