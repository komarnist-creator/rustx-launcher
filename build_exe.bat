@echo off
where cargo >nul 2>nul
if errorlevel 1 (
  echo Cargo ne najden. Ustanovite Rust: https://rustup.rs
  pause
  exit /b 1
)
cargo build --release
if errorlevel 1 (
  echo Oshibka sborki.
  pause
  exit /b 1
)
copy /Y target\release\rustx.exe rustx.exe
echo.
echo Gotovo: rustx.exe lezhit v etoj papke.
pause
