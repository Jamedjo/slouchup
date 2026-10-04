# Builds target/release/slouchup-windows.zip holding slouchup.exe. Run from the repository root.
$ErrorActionPreference = "Stop"
cargo build --release -p slouchup
if ($LASTEXITCODE) { exit $LASTEXITCODE }
Compress-Archive -Force -Path target/release/slouchup.exe -DestinationPath target/release/slouchup-windows.zip
