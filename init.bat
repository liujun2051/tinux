@echo off
cd /d "%~dp0"
rem tinux init: assemble the winbox runtime (source repo excludes it).
rem Requires: curl + tar (built into Win10); needs network access.
setlocal

if exist winbox\bin\busybox.exe (
    echo winbox runtime already exists, skipping downloads.
    echo Delete winbox\ to rebuild from scratch.
    goto :shim
)

echo [1/4] Downloading busybox (x86_64 Unicode + ANSI)...
mkdir winbox\bin\nodejs winbox\app winbox\usr\lib 2>nul
curl -fsSL -o winbox\bin\busybox.exe https://frippery.org/files/busybox/busybox64u.exe
if errorlevel 1 goto :err
curl -fsSL -o winbox\bin\busybox-ansi.exe https://frippery.org/files/busybox/busybox64.exe
if errorlevel 1 goto :err

echo [2/4] Downloading Python (embeddable)...
curl -fsSL -o "%TEMP%\python-embed.zip" https://www.python.org/ftp/python/3.12.10/python-3.12.10-embed-amd64.zip
if errorlevel 1 goto :err
tar -xf "%TEMP%\python-embed.zip" -C winbox\bin
del "%TEMP%\python-embed.zip" 2>nul

echo [3/4] Downloading Node.js...
curl -fsSL -o "%TEMP%\node.zip" https://nodejs.org/dist/v25.9.0/node-v25.9.0-win-x64.zip
if errorlevel 1 goto :err
tar -xf "%TEMP%\node.zip" -C winbox\bin
ren winbox\bin\node-v25.9.0-win-x64 nodejs
del "%TEMP%\node.zip" 2>nul

:shim
echo [4/4] Installing mini-linux shim...
mkdir winbox\usr\lib 2>nul
copy /y runtime\minilinux.sh winbox\usr\lib\minilinux.sh >nul

echo.
echo Done! Run start.bat to launch tinux.
exit /b 0

:err
echo Download failed. Check your network and retry.
exit /b 1
