@echo off
rem tinux telnetd self-contained daemon manager (no Windows service dependency)
rem usage: telnetd.bat start [port] | stop | status
setlocal enabledelayedexpansion
cd /d "%~dp0.."
set PY=bin\python\pythonw.exe
if not exist %PY% (
  echo [telnetd] python not found: %PY%
  exit /b 1
)
set CMD=%~1
if "%CMD%"=="" set CMD=start
if "%CMD%"=="start" (
  if not "%~2"=="" (
    %PY% bin\telnetd.py --daemon %2
  ) else (
    %PY% bin\telnetd.py --daemon
  )
  echo [telnetd] started
  goto :eof
)
if "%CMD%"=="stop" (
  if exist app\telnetd.pid (
    set /p PID=<app\telnetd.pid
    taskkill /F /PID !PID! >nul 2>&1
    del app\telnetd.pid 2>nul
    echo [telnetd] stopped pid !PID!
  ) else (
    echo [telnetd] not running
  )
  goto :eof
)
if "%CMD%"=="status" (
  if exist app\telnetd.pid (
    set /p PID=<app\telnetd.pid
    tasklist /FI "PID eq !PID!" | findstr /I "!PID!" >nul && (echo [telnetd] running pid !PID!) || (echo [telnetd] stale pidfile)
  ) else (
    echo [telnetd] not running
  )
  goto :eof
)
echo usage: telnetd.bat start [port] ^| stop ^| status
