# Installs, verifies, and uninstalls the built setup package.
#
# Compiling an .iss proves the script parses; it proves nothing about whether
# the resulting installer lays files down correctly, registers an uninstaller,
# or cleans up after itself. This exercises the round trip.
#
# Requires administrator rights (the installer is PrivilegesRequired=admin).

$ErrorActionPreference = 'Continue'
$proj = Split-Path $PSScriptRoot -Parent

# Read the version rather than hard-coding it: a hard-coded one turns the first
# release after a version bump into "setup not found; run iscc first", which
# reads as a broken build rather than a stale test.
#
# Anchored to the [package] block. A bare '^version = "..."' also matches the
# one under [dependencies.windows], and only picks the right line because
# [package] happens to come first today -- reordering Cargo.toml would silently
# start testing the windows-crate version instead.
function Get-PackageVersion($manifest) {
  $inPackage = $false
  foreach ($line in Get-Content $manifest) {
    if ($line -match '^\s*\[(.+)\]\s*$') { $inPackage = ($Matches[1] -eq 'package'); continue }
    if ($inPackage -and $line -match '^\s*version\s*=\s*"([^"]+)"') { return $Matches[1] }
  }
  return $null
}

$version = Get-PackageVersion (Join-Path $proj 'Cargo.toml')
if (-not $version) { Write-Host 'RESULT: FAIL - could not read [package] version from Cargo.toml'; exit 1 }
Write-Host ("testing version {0}" -f $version)
$setup = Join-Path $proj "dist\Cantos-Setup-$version.exe"
$appId   = '{7B1F4C2E-9A3D-4E58-B6C1-2F8A5D0E7341}_is1'
$runKey  = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
         ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) { Write-Host 'RESULT: SKIP - needs administrator rights'; exit 0 }
if (-not (Test-Path $setup)) { Write-Host "RESULT: FAIL - $setup not found; run iscc first"; exit 1 }

function UninstallKey {
  foreach ($h in @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\$appId",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\$appId")) {
    if (Test-Path $h) { return Get-ItemProperty $h }
  }
  return $null
}

if (UninstallKey) { Write-Host 'RESULT: SKIP - Cantos is already installed; uninstall it first'; exit 0 }

Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 600

Write-Host '--- install (silent) ---'
$p = Start-Process -FilePath $setup -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/SP-' -Wait -PassThru
Write-Host ("  exit code: {0}" -f $p.ExitCode)

$key = UninstallKey
$installed = $null
if ($key) { $installed = Join-Path $key.InstallLocation 'cantos.exe' }

$okExit    = $p.ExitCode -eq 0
$okKey     = $null -ne $key
$okFile    = $installed -and (Test-Path $installed)
$okName    = $key -and $key.DisplayName -like 'Cantos*'
$okUninst  = $key -and $key.UninstallString
Write-Host ("  uninstall entry:  {0}" -f ($(if ($okKey) { $key.DisplayName + ' ' + $key.DisplayVersion } else { 'MISSING' })))
Write-Host ("  install location: {0}" -f ($(if ($okKey) { $key.InstallLocation } else { 'n/a' })))
Write-Host ("  binary present:   {0}" -f $okFile)

$shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Cantos\Cantos.lnk'
$okLnk = Test-Path $shortcut
Write-Host ("  start menu entry: {0}" -f $okLnk)

Write-Host '--- run the installed binary ---'
$okRun = $false
if ($okFile) {
  Start-Process -FilePath $installed
  Start-Sleep -Seconds 3
  $proc = Get-Process cantos -ErrorAction SilentlyContinue
  $okRun = $null -ne $proc
  if ($okRun) {
    Write-Host ("  running: pid={0}  {1:N1} MB private" -f $proc.Id, ($proc.PrivateMemorySize64/1MB))
    Write-Host ("  image:   {0}" -f $proc.Path)
  } else { Write-Host '  did not start' }
  Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 800
}

Write-Host '--- uninstall (silent) ---'
$okGone = $false; $okKeyGone = $false; $okRunGone = $false; $okProfileGone = $false
if ($okUninst) {
  # Inno records this quoted; Start-Process wants it unquoted.
  $u = $key.UninstallString.Trim('"')
  $up = Start-Process -FilePath $u -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Wait -PassThru
  Write-Host ("  exit code: {0}" -f $up.ExitCode)
  Start-Sleep -Seconds 2
  $okGone    = -not (Test-Path $installed)
  $okKeyGone = $null -eq (UninstallKey)
  $okRunGone = -not (Get-ItemProperty -Path $runKey -Name 'Cantos' -ErrorAction SilentlyContinue)
  # The WebView2 profile lives under LOCALAPPDATA because the app cannot write
  # beside its exe in Program Files. The installer never creates it, so nothing
  # cleans it up unless [Code] does -- several MB of Chromium cache was being
  # left behind on every uninstall.
  $okProfileGone = -not (Test-Path (Join-Path $env:LOCALAPPDATA 'Cantos'))
  Write-Host ("  binary removed:        {0}" -f $okGone)
  Write-Host ("  uninstall key removed: {0}" -f $okKeyGone)
  Write-Host ("  HKCU Run entry clear:  {0}" -f $okRunGone)
  Write-Host ("  WebView2 profile gone: {0}" -f $okProfileGone)
}

# Settings are intentionally preserved across uninstall.
$cfg = Join-Path $env:APPDATA 'Cantos\config.json'
Write-Host ("  config preserved:      {0} (by design)" -f (Test-Path $cfg))

Write-Host ''
$all = $okExit -and $okKey -and $okFile -and $okName -and $okLnk -and $okRun -and $okGone -and $okKeyGone -and $okRunGone -and $okProfileGone
if ($all) { Write-Host 'RESULT: PASS - install, run, and uninstall all clean'; exit 0 }
Write-Host ("RESULT: FAIL  exit={0} key={1} file={2} name={3} lnk={4} run={5} removed={6} keyGone={7} runGone={8} profileGone={9}" `
  -f $okExit, $okKey, $okFile, $okName, $okLnk, $okRun, $okGone, $okKeyGone, $okRunGone, $okProfileGone)
exit 1
