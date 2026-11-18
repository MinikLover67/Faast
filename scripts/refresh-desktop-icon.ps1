$ErrorActionPreference = 'Stop'
$exe = "$env:LOCALAPPDATA\Faast\faast.exe"
if (-not (Test-Path $exe)) { throw "Faast exe not found at $exe" }

Write-Host 'Stopping Explorer...'
Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
Start-Sleep -Seconds 2

Write-Host 'Deleting icon caches...'
$files = @("$env:LOCALAPPDATA\IconCache.db") + @(Get-ChildItem "$env:LOCALAPPDATA\Microsoft\Windows\Explorer" -Filter 'iconcache*.db' -Force -ErrorAction SilentlyContinue | Select-Object -ExpandProperty FullName)
foreach ($f in $files) { Remove-Item -LiteralPath $f -Force -ErrorAction SilentlyContinue }
Write-Host "  removed $($files.Count) cache files"

Write-Host 'Recreating shortcuts with explicit icon...'
$desktop = [Environment]::GetFolderPath('Desktop')
$start = "$env:APPDATA\Microsoft\Windows\Start Menu\Programs"
$ws = New-Object -ComObject WScript.Shell
foreach ($p in @("$desktop\Faast.lnk", "$start\Faast.lnk")) {
  Remove-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
  $s = $ws.CreateShortcut($p)
  $s.TargetPath = $exe
  $s.WorkingDirectory = Split-Path $exe
  $s.IconLocation = "$exe,0"
  $s.Description = 'Faast Download Manager'
  $s.Save()
  Write-Host "  $p"
}

Write-Host 'Flushing icon cache...'
foreach ($arg in @('-show', '-ClearIconCache')) {
  $p = Start-Process -FilePath "$env:WINDIR\system32\ie4uinit.exe" -ArgumentList $arg -Wait -PassThru -ErrorAction SilentlyContinue
  if ($p) { Write-Host "  ie4uinit $arg exit=$($p.ExitCode)" }
}

Write-Host 'Restarting Explorer...'
Start-Process explorer.exe
Start-Sleep -Seconds 4
Write-Host 'Explorer running:' ([bool](Get-Process explorer -ErrorAction SilentlyContinue))
