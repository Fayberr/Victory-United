@echo off
rem Opens the launcher window (launcher.ps1). Double-click it.
start "" powershell.exe -NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -STA -File "%~dp0launcher.ps1"
