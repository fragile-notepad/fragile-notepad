param(
    [Parameter(Mandatory = $true)]
    [string] $Installer
)
$ErrorActionPreference = 'Stop'
$destination = Join-Path $env:RUNNER_TEMP 'Fragile Notepad installed'
$install = Start-Process -FilePath (Resolve-Path $Installer).Path -ArgumentList @('/S', "/D=$destination") -WindowStyle Hidden -Wait -PassThru
if ($install.ExitCode -ne 0) { throw "Installer failed: $($install.ExitCode)" }
try {
    python "$PSScriptRoot/../check_package.py" --executable "$destination/fragile-notepad.exe"
    if ($LASTEXITCODE -ne 0) { throw 'Installed editor failed to start' }
    if (!(Test-Path "$destination/file-type-icons/rust.ico")) { throw 'Missing document icon' }
    $icon = (Get-ItemProperty 'HKLM:/Software/Classes/FragileNotepad.rust/DefaultIcon').'(default)'
    if (!$icon.Contains('rust.ico')) { throw 'Missing document icon registration' }
} finally {
    Start-Process -FilePath "$destination/Uninstall.exe" -ArgumentList '/S' -WindowStyle Hidden -Wait
}
$deadline = (Get-Date).AddSeconds(30)
while ((Test-Path "$destination/fragile-notepad.exe") -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
if (Test-Path "$destination/fragile-notepad.exe") { throw 'Uninstaller left the editor behind' }
if (Test-Path 'HKLM:/Software/Classes/FragileNotepad.rust') { throw 'Uninstaller left file associations behind' }
