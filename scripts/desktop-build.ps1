$ErrorActionPreference = 'Stop'
$ProjectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $ProjectRoot
try {
    cargo build --locked --release -p agentkit-cli
    if ($LASTEXITCODE -ne 0) { throw 'Rust CLI build failed' }
    $Metadata = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot locate Cargo build output' }
    $EngineDirectory = Join-Path $ProjectRoot 'apps/desktop/src-tauri/binaries'
    New-Item -ItemType Directory -Force -Path $EngineDirectory | Out-Null
    Copy-Item (Join-Path $Metadata.target_directory 'release/agentkit.exe') (Join-Path $EngineDirectory 'agentkit.exe')
    npm --prefix (Join-Path $ProjectRoot 'apps/desktop') run tauri build
    if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed' }
} finally {
    Pop-Location
}
