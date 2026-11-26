@echo off
setlocal
if /i "%~1"=="--help" goto help
if /i "%~1"=="-h" goto help

set "REPO=%~dp0."
set "EXE=%REPO%\target\release\victory.exe"
set "GAME=%~dp0..\GTA6-Xbox-v1.0.2.1\game_scarlett_master.exe"
set "VICTORY_CONFIG=%REPO%\victory.toml"
if not exist "%EXE%" (
    echo [ERROR] Build victory-curated with update-victory-curated.bat first.
    exit /b 1
)
if not exist "%GAME%" (
    echo [ERROR] Could not find "%GAME%".
    exit /b 1
)
if not exist "%VICTORY_CONFIG%" (
    echo [ERROR] Could not find "%VICTORY_CONFIG%".
    exit /b 1
)
cd /d "%REPO%" || exit /b 1

if /i "%~1"=="--bind-only" goto bind
if /i "%~1"=="--raw" goto raw
if /i "%~1"=="--series-x" goto profile
if /i "%~1"=="--prewarm" goto profile
if /i "%~1"=="--no-save-encryption" goto nosaveenc
if "%~1"=="" goto profile
echo [ERROR] Unknown option "%~1". Use --help.
exit /b 2

:profile
set "VICTORY_LEVELS_ON_DEMAND=1"
set "VICTORY_CARD_BUFFERS_MIN_MB=1"
set "VICTORY_CARD_BUFFERS_MB=2048"
set "VICTORY_RELAY=2"
set "VICTORY_LATE_WAIT=3"
set "VICTORY_WIDE_BACK=1"
set "VICTORY_LIBRARY_TRIM=0"
set "VICTORY_REBAR=0"
set "VICTORY_COPY_BACK_ONLY_READBACK=0"
set "VICTORY_AMBIENT_STUB=1"
set "VICTORY_SHADOWMASK_STUB=1"
set "VICTORY_EXPOSURE_STUB=1"

:nosaveenc
:: Lets a story save that was imported from another platform load. Only in the
:: loaded copy; the game files stay as they are.
set "VICTORY_PATCH=0x28d0d60=31c0c3:ffc9f7c1"

if /i "%~1"=="--prewarm" goto prewarm
if /i "%~1"=="--series-x" goto series_x
set "VICTORY_OWN_UPSCALER=1"
set "VICTORY_RENDER_SCALE=0.5"
"%EXE%" run --console series-s --execute "%GAME%"
exit /b %errorlevel%

:series_x
"%EXE%" run --console series-x --execute "%GAME%" %2 %3 %4 %5 %6 %7 %8 %9
exit /b %errorlevel%

:bind
"%EXE%" run "%GAME%"
exit /b %errorlevel%

:raw
"%EXE%" run --execute "%GAME%" %2 %3 %4 %5 %6 %7 %8 %9
exit /b %errorlevel%

:prewarm
"%EXE%" precompile --pipelines --jobs 16 --pipeline-jobs 12 %2 %3 %4 %5 %6 %7 %8 %9
exit /b %errorlevel%

:help
echo Usage: launch-victory-curated.bat [option]
echo   no option    Series S performance profile with 0.5 render scale
echo   --series-x   Series X with the curated performance flags
echo   --raw        Launch with the binary's default settings
echo   --prewarm    Fill the curated shader and compute pipeline caches
echo   --bind-only  Map the executable and check imports without running it
  echo   --no-save-encryption  Also let a converted story save load
exit /b 0
