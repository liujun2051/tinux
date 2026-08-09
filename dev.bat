@echo off
rem feier-two dev launcher
rem GNU toolchain (stable-x86_64-pc-windows-gnu) needs mingw-w64 binutils
rem from MSYS2 mingw64: dlltool / windres / gcc / ar / as
set "PATH=C:\msys64\mingw64\bin;%PATH%"
npm run tauri dev
