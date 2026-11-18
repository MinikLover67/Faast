# Signs the NSIS installer with a code-signing certificate.
# Requires signtool.exe (Windows SDK) and a PFX certificate.
# Usage:
#   $env:FAST_SIGN_PFX='C:\cert.pfx'
#   $env:FAST_SIGN_PASSWORD='...'
#   powershell -File scripts\sign-installer.ps1
$ErrorActionPreference = 'Stop'

$pfx = $env:FAST_SIGN_PFX
$pass = $env:FAST_SIGN_PASSWORD
if (-not $pfx -or -not (Test-Path $pfx)) { throw 'Set FAST_SIGN_PFX to a valid .pfx path' }
if (-not $pass) { throw 'Set FAST_SIGN_PASSWORD' }

$installer = 'src-tauri\target\release\bundle\nsis\Faast_0.1.0_x64-setup.exe'
if (-not (Test-Path $installer)) { throw "Installer not found: $installer" }

$signtool = Get-Command signtool.exe -ErrorAction SilentlyContinue
if (-not $signtool) {
  $candidates = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match 'x64' } | Select-Object -First 1
  if ($candidates) { $signtool = $candidates.FullName } else { throw 'signtool.exe not found; install the Windows SDK' }
} else { $signtool = $signtool.Source }

Write-Host "Signing $installer"
& $signtool sign /f $pfx /p $pass /tr 'http://timestamp.digicert.com' /td sha256 /fd sha256 $installer
if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE" }

& $signtool verify /pa $installer
Write-Host 'Signature verified.'
