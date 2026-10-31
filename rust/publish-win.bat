@echo off
REM Publish the Rust build for Windows x64: one self-contained exe (FFDec is embedded and
REM unpacked into Tools\ on first launch) plus the default config.ini.
REM Output: publish\rust-win-x64\habbo-downloader.exe
REM Needs Rust (rustup) and, for the GNU toolchain, MinGW-w64 on PATH.

setlocal
set OUT=%~dp0..\publish\rust-win-x64

pushd "%~dp0"
cargo build --release -p habbo-app
set BUILD_EXIT=%ERRORLEVEL%
popd
if not "%BUILD_EXIT%"=="0" exit /b %BUILD_EXIT%

if exist "%OUT%" rmdir /s /q "%OUT%"
mkdir "%OUT%"
copy /y "%~dp0target\release\habbo-downloader.exe" "%OUT%\" >nul
copy /y "%~dp0crates\core\assets\default_config.ini" "%OUT%\config.ini" >nul

echo.
echo Published Windows build at: %OUT%
echo Run: "%OUT%\habbo-downloader.exe"
