$ErrorActionPreference = "Stop"

$repoRoot = (git rev-parse --show-toplevel 2>$null)
if (-not $repoRoot) {
    $repoRoot = Split-Path -Parent $PSScriptRoot
}

Set-Location $repoRoot

function Invoke-CiCommand {
    param([Parameter(ValueFromRemainingArguments = $true)] [string[]] $Command)

    & $Command[0] $Command[1..($Command.Length - 1)]
    if ($LASTEXITCODE -ne 0) {
        throw "$($Command -join ' ') failed with exit code $LASTEXITCODE"
    }
}

Invoke-CiCommand -Command @("cargo", "fmt", "--package", "fragile-notepad", "--check")
Invoke-CiCommand -Command @("python", "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*font_profiles.py")
Invoke-CiCommand -Command @("python", "scripts/prepare_font_profiles.py", "--out-dir", "target/font-profiles/prepared", "--target-os", "windows", "--cache-dir", "target/font-profiles")
& .\scripts\generate_icon_assets.ps1
Invoke-CiCommand -Command @("python", "scripts/test_icon_assets.py")
Invoke-CiCommand -Command @("cargo", "clippy", "--locked", "--all-targets")
# cargo test also compiles the application and examples.
Invoke-CiCommand -Command @("cargo", "test")
Invoke-CiCommand -Command @("cargo", "test", "--locked", "--package", "iced_wgpu", "--lib")
Invoke-CiCommand -Command @("cargo", "test", "--locked", "--package", "cryoglyph", "--lib")
Invoke-CiCommand -Command @("cargo", "check", "--no-default-features")
