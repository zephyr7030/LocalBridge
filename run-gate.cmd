@echo off
setlocal enabledelayedexpansion
cd /d "%~dp0"

echo === 0/3 rustfmt on changed Rust files (same command the gate checks with) ===
for /f "delims=" %%f in ('git diff --name-only --diff-filter=ACMR HEAD -- "*.rs"') do (
  echo   fmt %%f
  rustfmt --edition 2024 --config skip_children=true "%%f"
)

echo.
echo === 1/3 shared gate ===
node scripts\test\ci-gate.mjs > ci-gate.log 2>&1
set GATE=%ERRORLEVEL%
echo EXIT_CODE=%GATE%>> ci-gate.log
if "%GATE%"=="0" (echo gate PASSED) else (echo gate FAILED exit=%GATE% - see ci-gate.log)

echo.
echo === 2/3 fixed-window E2E (real Tauri window) ===
node tests\e2e\onboarding\fixed_window_runtime_e2e.mjs > e2e.log 2>&1
set E2E=%ERRORLEVEL%
echo EXIT_CODE=%E2E%>> e2e.log
if "%E2E%"=="0" (echo e2e PASSED) else (echo e2e FAILED exit=%E2E% - see e2e.log)

echo.
echo Logs: ci-gate.log, e2e.log
pause
