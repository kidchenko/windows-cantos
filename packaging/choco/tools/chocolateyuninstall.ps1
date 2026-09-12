$ErrorActionPreference = 'Stop'

$key = Get-UninstallRegistryKey -SoftwareName 'Cantos*'

if (-not $key) {
  Write-Warning 'Cantos is not registered as installed; nothing to remove.'
  return
}
if ($key.Count -gt 1) {
  Write-Warning "Found $($key.Count) matching entries; skipping to avoid removing the wrong one."
  $key | ForEach-Object { Write-Warning "  $($_.DisplayName)" }
  return
}

Uninstall-ChocolateyPackage -PackageName 'cantos' -FileType 'EXE' `
  -SilentArgs '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART' `
  -ValidExitCodes @(0) `
  -File $key.UninstallString.Trim('"')
