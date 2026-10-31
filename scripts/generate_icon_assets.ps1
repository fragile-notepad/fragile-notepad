$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

python .\scripts\rasterize_illustrations.py
if ($LASTEXITCODE -ne 0) { throw "Illustration generation failed" }
python .\scripts\generate_app_icons.py
if ($LASTEXITCODE -ne 0) { throw "Application icon generation failed" }
python .\scripts\generate_file_type_icons.py
if ($LASTEXITCODE -ne 0) { throw "File-type icon generation failed" }
