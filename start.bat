@echo off
cd /d "%~dp0"
rem tinux start: assemble runtime if missing, then launch the dev app.
if not exist winbox\bin\busybox.exe (
    echo First run: assembling winbox runtime...
    call init.bat
    if errorlevel 1 exit /b 1
)
rem GNU toolchain build needs MSYS2 mingw-w64 binutils (dlltool/windres/gcc/ar/as)
set "PATH=C:\msys64\mingw64\bin;%PATH%"
npm run tauri dev
