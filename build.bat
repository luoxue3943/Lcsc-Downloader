@echo off
setlocal
pushd "%~dp0" || exit /b 1
cargo build --release --locked
set "BUILD_EXIT_CODE=%ERRORLEVEL%"
popd
exit /b %BUILD_EXIT_CODE%
