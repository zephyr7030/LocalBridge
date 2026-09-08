@echo off
setlocal
cd /d "%~dp0"

echo 只做类型检查，不优化、不链接、不打包——回答"能不能编译"用不着构建发布版。
echo.

cargo check --manifest-path src-tauri\Cargo.toml --all-targets > rust-check.log 2>&1
set RC=%ERRORLEVEL%
echo EXIT_CODE=%RC%>> rust-check.log

if "%RC%"=="0" (
  echo 编译通过。完整门禁请跑 run-gate.cmd。
) else (
  echo 编译失败，诊断在 rust-check.log
  echo.
  findstr /B /C:"error" rust-check.log
)
echo.
pause
