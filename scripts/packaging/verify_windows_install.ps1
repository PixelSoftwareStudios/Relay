# Install the MSI quietly, check the payload against the built binaries, then uninstall.
# Does not start the application.
param(
    [Parameter(Mandatory = $true)][string]$Installer,
    [Parameter(Mandatory = $true)][string]$BinDir
)
$ErrorActionPreference = 'Stop'
$install = Join-Path $env:ProgramFiles 'Relay'
$log = "msi-$([IO.Path]::GetFileNameWithoutExtension($Installer)).log"

function Invoke-Msi([string[]]$Arguments) {
    $process = Start-Process msiexec.exe -ArgumentList $Arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "msiexec $($Arguments -join ' ') exited with $($process.ExitCode)" }
}

Invoke-Msi @('/i', "`"$Installer`"", '/qn', '/l*v', $log)
foreach ($name in 'relay-desktop.exe', 'relay.exe', 'relayd.exe') {
    $installed = Join-Path $install $name
    if (-not (Test-Path $installed)) { throw "Missing installed file: $installed" }
    $expected = (Get-FileHash (Join-Path $BinDir $name) -Algorithm SHA256).Hash
    if ((Get-FileHash $installed -Algorithm SHA256).Hash -ne $expected) { throw "$name differs from the release binary" }
}
foreach ($name in 'LICENSE', 'THIRD_PARTY_LICENSES') {
    if (-not (Test-Path (Join-Path $install "doc\$name"))) { throw "Missing licence: $name" }
}
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
if (($machinePath -split ';') -notcontains $install) { throw 'Install folder is not on the machine PATH' }

# Repair restores a deleted file.
Remove-Item (Join-Path $install 'relayd.exe')
Invoke-Msi @('/fa', "`"$Installer`"", '/qn', '/l*v', $log)
if (-not (Test-Path (Join-Path $install 'relayd.exe'))) { throw 'Repair did not restore relayd.exe' }

Invoke-Msi @('/x', "`"$Installer`"", '/qn', '/l*v', $log)
if (Test-Path $install) { throw 'Uninstall left the install folder behind' }
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
if (($machinePath -split ';') -contains $install) { throw 'Uninstall left the PATH entry behind' }
Write-Host 'Verified MSI install, repair and uninstall'
