<#
.SYNOPSIS
  Builds cantos.exe and the Inno Setup installer.

.EXAMPLE
  .\build.ps1                 # binary + installer
  .\build.ps1 -Run            # ... then launch it
  .\build.ps1 -Test           # ... then run the full test suite (needs admin)
  .\build.ps1 -SkipInstaller  # binary only, no Inno Setup needed
#>
[CmdletBinding()]
param(
  [switch]$Run,
  [switch]$Test,
  [switch]$SkipInstaller
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

function Step($m) { Write-Host "`n==> $m" -ForegroundColor Cyan }
function Ok($m)   { Write-Host "    $m" -ForegroundColor Green }
function Warn($m) { Write-Host "    $m" -ForegroundColor Yellow }

# Resolve tools by PATH first, then by their known install locations. A shell
# opened before rustup ran will not have cargo on PATH even though the
# persisted user PATH contains it, and Inno Setup never adds itself.
function Find($name, $fallbacks) {
  $c = Get-Command $name -ErrorAction SilentlyContinue
  if ($c) { return $c.Source }
  foreach ($f in $fallbacks) { if (Test-Path $f) { return $f } }
  return $null
}

$cargo = Find 'cargo' @("$env:USERPROFILE\.cargo\bin\cargo.exe")
if (-not $cargo) {
  Write-Host "cargo not found. Install the Rust MSVC toolchain:" -ForegroundColor Red
  Write-Host "  winget install Rustlang.Rustup"
  Write-Host "  rustup default stable-x86_64-pc-windows-msvc"
  exit 1
}

Step 'Building release binary'
& $cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
$exe = Join-Path $PSScriptRoot 'target\release\cantos.exe'
Ok ("cantos.exe   {0:N2} MB" -f ((Get-Item $exe).Length / 1MB))

if (-not $SkipInstaller) {
  $iscc = Find 'iscc' @(
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe")
  if (-not $iscc) {
    Warn 'Inno Setup not found; skipping the installer.'
    Warn 'Install it with:  choco install innosetup -y'
    Warn 'or from https://jrsoftware.org/isinfo.php'
  } else {
    Step 'Building installer'
    # Inno cannot overwrite a setup .exe that Explorer or a scanner still has
    # open, and a stale one is worse than none.
    Remove-Item 'dist\Cantos-Setup-*.exe' -ErrorAction SilentlyContinue
    & $iscc /Qp 'packaging\inno\cantos.iss'
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed ($LASTEXITCODE)" }
    Get-ChildItem 'dist\Cantos-Setup-*.exe' | ForEach-Object {
      Ok ("{0}   {1:N2} MB" -f $_.Name, ($_.Length / 1MB))
    }
  }
}

if ($Test) {
  Step 'Unit tests'
  & $cargo test
  if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

  $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
           ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

  Step 'Integration tests'
  Warn 'These move the cursor, open Task View, and minimise windows. Leave the mouse alone.'
  # Collected rather than thrown on immediately: every script restores the
  # config and the cursor on its way out, so there is no reason to stop the
  # run early, and seeing all the failures at once beats finding them one
  # re-run at a time.
  $failed = @()
  foreach ($t in 'e2e', 'drag', 'fullscreen', 'actions') {
    Write-Host "`n--- tests\$t.ps1 ---"
    & powershell -NoProfile -ExecutionPolicy Bypass -File "tests\$t.ps1"
    # Each script exits non-zero on RESULT: FAIL. Without this check a real
    # regression is just a line of text in the middle of a run that then
    # reports success.
    if ($LASTEXITCODE -ne 0) { $failed += $t }
  }
  Write-Host "`n--- tests\installer.ps1 ---"
  if ($admin) {
    & powershell -NoProfile -ExecutionPolicy Bypass -File 'tests\installer.ps1'
    if ($LASTEXITCODE -ne 0) { $failed += 'installer' }
  }
  else { Warn 'Skipped: needs an elevated shell.' }

  if ($failed.Count) { throw ("integration tests failed: {0}" -f ($failed -join ', ')) }
  Ok 'All integration tests passed.'
}

if ($Run) {
  Step 'Launching'
  Stop-Process -Name cantos -Force -ErrorAction SilentlyContinue
  Start-Sleep -Milliseconds 500
  Start-Process $exe -ArgumentList '--settings'
  Ok 'Running in the tray; settings window opened.'
}

Write-Host ''
