@echo off
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust/Cargo not found. Install Rust from https://rustup.rs/ and reopen this terminal.
  pause
  exit /b 1
)
if not exist "PresentMon.exe" (
  echo PresentMon.exe is missing. Download it with:
  echo powershell -ExecutionPolicy Bypass -File scripts\download-presentmon.ps1
  pause
  exit /b 1
)
cargo build --release
if errorlevel 1 (
  echo Build failed.
  pause
  exit /b 1
)
copy /Y "PresentMon.exe" "target\release\PresentMon.exe" >nul
copy /Y "README-fa.md" "target\release\README-fa.md" >nul
echo.
echo Done: target\release\zoro-fps-overlay.exe
echo Keep PresentMon.exe in the same folder as the app.
pause
