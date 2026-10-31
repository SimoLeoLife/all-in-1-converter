@echo off
REM Publish the Windows x64 build: one self-contained exe (FFDec is embedded and unpacked into
REM Tools\ on first launch) plus the default config.ini.
REM Output: publish\win-x64\habbo-downloader.exe
REM Needs Rust (rustup) and, for the GNU toolchain, MinGW-w64 on PATH.

setlocal
set ROOT=%~dp0..
set OUT=%ROOT%\publish\win-x64

pushd "%ROOT%"
cargo build --release -p habbo-app
set BUILD_EXIT=%ERRORLEVEL%
popd
if not "%BUILD_EXIT%"=="0" exit /b %BUILD_EXIT%

if exist "%OUT%" rmdir /s /q "%OUT%"
mkdir "%OUT%"
copy /y "%ROOT%\target\release\habbo-downloader.exe" "%OUT%\" >nul
copy /y "%ROOT%\crates\core\assets\default_config.ini" "%OUT%\config.ini" >nul

echo.
echo Published Windows build at: %OUT%
echo Run: "%OUT%\habbo-downloader.exe"
