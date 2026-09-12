# Shared helpers for the integration scripts.
#
# Dot-source this: . "$PSScriptRoot\common.ps1"

$script:HcConfigDir = Join-Path $env:APPDATA 'Cantos'
$script:HcConfig    = Join-Path $script:HcConfigDir 'config.json'

# What Backup-HcConfig established, and therefore what Restore-HcConfig is
# entitled to do. Tracked explicitly rather than inferred from whether the
# backup file exists: "no backup on disk" means both "there was nothing to
# save" and "we already restored", and those call for opposite actions --
# deleting the config, or leaving it alone.
#   $null          not backed up yet, or already restored -> do nothing
#   'backed-up'    a backup exists                        -> copy it back
#   'none-existed' there was no config before             -> remove ours
$script:HcBackupState = $null

function Get-HcConfigPath { $script:HcConfig }

# Back up the live config before a test overwrites it.
#
# Refuses to run if a backup from a previous test is still sitting there.
# That state means an earlier run died before restoring, so the live config is
# already a test config -- and backing up again would overwrite the only copy
# of the user's real settings with a throwaway one. It has happened; the
# recovery is to restore the stale backup by hand, which the message explains.
function Backup-HcConfig($backup) {
  if (Test-Path $backup) {
    Write-Host ''
    Write-Host 'REFUSING TO RUN: a backup from a previous test run is still present.' -ForegroundColor Red
    Write-Host ("  {0}" -f $backup)
    Write-Host 'That run did not restore, so the live config is probably a test config'
    Write-Host 'and this backup holds your real settings. Backing up again would destroy'
    Write-Host 'them. Restore it first:'
    Write-Host ''
    Write-Host ("  Copy-Item '{0}' '{1}' -Force" -f $backup, $script:HcConfig) -ForegroundColor Yellow
    Write-Host ("  Remove-Item '{0}'" -f $backup) -ForegroundColor Yellow
    Write-Host ''
    return $false
  }
  New-Item -ItemType Directory -Force -Path $script:HcConfigDir | Out-Null

  if (-not (Test-Path $script:HcConfig)) {
    $script:HcBackupState = 'none-existed'
    Write-Host 'no live config to back up'
    return $true
  }

  Copy-Item $script:HcConfig $backup -Force
  # Verify rather than assume. These scripts run with
  # $ErrorActionPreference = 'Continue', so a failed copy would sail straight
  # past -- the test would overwrite the live config and the restore would
  # then find no backup, which is exactly the settings loss this helper
  # exists to prevent.
  if (-not (Test-Path $backup)) {
    Write-Host ''
    Write-Host 'REFUSING TO RUN: could not write the config backup.' -ForegroundColor Red
    Write-Host ("  {0}" -f $backup)
    Write-Host 'Running without one risks losing your settings.'
    Write-Host ''
    return $false
  }
  $script:HcBackupState = 'backed-up'
  Write-Host ("backed up the live config to {0}" -f (Split-Path $backup -Leaf))
  return $true
}

# Put the live config back. Idempotent: a second call is a no-op rather than
# deleting the config the first call just restored.
function Restore-HcConfig($backup) {
  switch ($script:HcBackupState) {
    'backed-up' {
      Copy-Item $backup $script:HcConfig -Force
      Remove-Item $backup -Force
      Write-Host 'restored the original config'
    }
    'none-existed' {
      # Nothing was there before the test, so leave nothing behind.
      Remove-Item $script:HcConfig -ErrorAction SilentlyContinue
    }
    default { }  # never backed up, or already restored
  }
  $script:HcBackupState = $null
}

# Write a test config as UTF-8 *without* a BOM.
#
# WriteAllText rather than Set-Content: Windows PowerShell 5.1 emits a BOM, and
# while the app strips one on read, a test should exercise the ordinary path.
function Write-HcConfig($table) {
  $json = $table | ConvertTo-Json -Depth 6
  [System.IO.File]::WriteAllText($script:HcConfig, $json, (New-Object System.Text.UTF8Encoding $false))
}

# The union of every monitor rect -- what OuterCorners anchors to.
# Returns @{ L; T; R; B } or $null if the enumeration came back nonsense.
function Get-HcUnionRect($rects) {
  $L = [int]::MaxValue; $T = [int]::MaxValue; $R = [int]::MinValue; $B = [int]::MinValue
  foreach ($line in $rects) {
    $n = $line -split ','
    if ($n.Count -lt 4) { continue }
    $L = [Math]::Min($L, [int]$n[0]); $T = [Math]::Min($T, [int]$n[1])
    $R = [Math]::Max($R, [int]$n[2]); $B = [Math]::Max($B, [int]$n[3])
  }
  if ($R -le $L -or $B -le $T) { return $null }
  return @{ L = $L; T = $T; R = $R; B = $B }
}
