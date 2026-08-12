@echo off
setlocal EnableExtensions
cd /d "%~dp0"

if /I "%~1"=="--check" (
  call :check
  exit /b %ERRORLEVEL%
)

if /I "%~1"=="--print-command" (
  call :check
  if errorlevel 1 exit /b %ERRORLEVEL%
  echo node_modules\.bin\tauri.cmd dev
  exit /b 0
)

call :check
if errorlevel 1 (
  echo.
  echo LocalBridge prerequisites are incomplete. Run the reported fix, then retry.
  pause
  exit /b %ERRORLEVEL%
)

echo Starting LocalBridge...
call "%CD%\node_modules\.bin\tauri.cmd" dev
set "LOCALBRIDGE_EXIT=%ERRORLEVEL%"
if not "%LOCALBRIDGE_EXIT%"=="0" (
  echo.
  echo LocalBridge exited with code %LOCALBRIDGE_EXIT%.
  pause
)
exit /b %LOCALBRIDGE_EXIT%

:check
where node >nul 2>nul || (
  echo [FAIL] Node.js is not available on PATH.
  exit /b 10
)
where cargo >nul 2>nul || (
  echo [FAIL] Cargo is not available on PATH.
  exit /b 11
)
if not exist "%CD%\package.json" (
  echo [FAIL] package.json is missing.
  exit /b 12
)
if not exist "%CD%\src-tauri\Cargo.toml" (
  echo [FAIL] src-tauri\Cargo.toml is missing.
  exit /b 13
)
if not exist "%CD%\node_modules\.bin\tauri.cmd" (
  echo [FAIL] Local project dependencies are missing. Install them once, then retry.
  exit /b 14
)
if not exist "%CD%\runtime\python\python.exe" (
  echo [FAIL] Bundled Python runtime is missing.
  exit /b 15
)
if not exist "%CD%\runtime\coding-tools-mcp\coding_tools_mcp\__init__.py" (
  echo [FAIL] Bundled coding-tools runtime is missing.
  exit /b 16
)
if not exist "%CD%\runtime\tunnel-client\tunnel-client.exe" (
  echo [FAIL] Bundled tunnel client is missing.
  exit /b 17
)
echo LOCALBRIDGE_LAUNCHER_CHECK=PASS
exit /b 0
