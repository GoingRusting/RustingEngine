param(
    [string]$Archive = "rusting-engine-windows-x86_64.zip"
)

$ErrorActionPreference = "Stop"
$EngineDir = Split-Path -Parent $PSScriptRoot
$DistDir = Join-Path $EngineDir "dist"
$PackageDir = Join-Path $env:TEMP ("rusting-engine-" + [guid]::NewGuid())
$ContentDir = Join-Path $PackageDir "RustingEngine"

try {
    New-Item -ItemType Directory -Force -Path $DistDir, $ContentDir | Out-Null
    Copy-Item (Join-Path $EngineDir "target/release/editor.exe") $ContentDir
    Copy-Item (Join-Path $EngineDir "target/release/user_main.exe") $ContentDir
    Copy-Item (Join-Path $EngineDir "target/release/cook_scene.exe") $ContentDir
    Copy-Item (Join-Path $EngineDir "README.md") $ContentDir
    $ImageDir = Join-Path $ContentDir "docs/images"
    New-Item -ItemType Directory -Force -Path $ImageDir | Out-Null
    Copy-Item (Join-Path $EngineDir "docs/images/spaceCubes.jpg") $ImageDir
    Copy-Item (Join-Path $EngineDir "CONTRIBUTING.md") $ContentDir
    Copy-Item (Join-Path $EngineDir "architecture.md") $ContentDir
    Copy-Item (Join-Path $EngineDir "roadmap.md") $ContentDir
    Copy-Item (Join-Path $EngineDir "RELEASE.md") $ContentDir
    Copy-Item (Join-Path $EngineDir "editor_gui.md") $ContentDir
    Copy-Item (Join-Path $EngineDir "CHANGELOG.md") $ContentDir
    Copy-Item (Join-Path $EngineDir "LICENSE.md") $ContentDir
    Compress-Archive -Path $ContentDir -DestinationPath (Join-Path $DistDir $Archive)
    Write-Host "Created dist/$Archive"
}
finally {
    if (Test-Path $PackageDir) {
        Remove-Item -Recurse -Force $PackageDir
    }
}
