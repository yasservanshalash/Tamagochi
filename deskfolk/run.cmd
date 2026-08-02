@echo off
REM Double-click this to start Deskfolk. It just hands off to run.ps1, which
REM does the real work (batch quoting is too fragile for launching npm).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run.ps1"
if errorlevel 1 pause
