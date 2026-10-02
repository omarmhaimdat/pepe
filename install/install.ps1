# pepe's installer for Windows, served at https://pepe.mhaimdat.com/install.ps1
#
#   powershell -ExecutionPolicy Bypass -c "irm https://pepe.mhaimdat.com/install.ps1 | iex"
#
# Runs the release installer (built by cargo-dist; it puts pepe.exe in
# ~\.local\bin and adds that to PATH), then has the new pepe set up tab
# completion for PowerShell: `pepe completions --install`. Set
# PEPE_NO_COMPLETIONS=1 to skip that step.
$ErrorActionPreference = 'Stop'

$installer = if ($env:PEPE_INSTALLER_URL) { $env:PEPE_INSTALLER_URL } else { 'https://pepe.mhaimdat.com/latest/pepe-installer.ps1' }
Invoke-Expression (Invoke-RestMethod -Uri $installer)

if ($env:PEPE_NO_COMPLETIONS) { return }
$candidates = @()
if ($env:CARGO_DIST_FORCE_INSTALL_DIR) { $candidates += $env:CARGO_DIST_FORCE_INSTALL_DIR }
if ($env:XDG_BIN_HOME) { $candidates += $env:XDG_BIN_HOME }
$candidates += Join-Path $HOME '.local\bin'
foreach ($dir in $candidates) {
  $pepe = Join-Path $dir 'pepe.exe'
  if (Test-Path $pepe) {
    Write-Host ''
    try { & $pepe completions --install } catch { Write-Warning "tab completion wasn't set up; run: pepe completions --install" }
    return
  }
}
Write-Host "pepe is installed; run 'pepe completions --install' for tab completion"
