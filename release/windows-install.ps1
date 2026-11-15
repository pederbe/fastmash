param(
    [Parameter(Mandatory)][string]$Archive,
    [Parameter(Mandatory)][string]$ExpectedSha256,
    [Parameter(Mandatory)][string]$QualifiedDirectory,
    [Parameter(Mandatory)][string]$ExpectedSource,
    [Parameter(Mandatory)][string]$Prefix,
    [Parameter(Mandatory)][string]$Checker
)
# Install the already qualified archive. Never rebuild or substitute executables.
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion -lt [version]'7.4') { throw 'PowerShell 7.4+ is required' }
if ($ExpectedSha256 -notmatch '^[0-9a-f]{64}$' -or (Get-FileHash $Archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $ExpectedSha256) { throw 'Archive differs from qualified bytes' }
if (Test-Path $Prefix) { throw 'Use a fresh installation prefix' }
$label = (Get-Content "$QualifiedDirectory/archive-version.txt" -Raw).Trim()
$kind = (Get-Content "$QualifiedDirectory/archive-kind.txt" -Raw).Trim()
$name = "fastmash-v$label-x86_64-pc-windows-msvc"
New-Item -ItemType Directory $Prefix | Out-Null
& $Checker outside (Split-Path $PSScriptRoot -Parent) $Prefix
if ($LASTEXITCODE -ne 0) { throw 'Installation must be outside the source checkout' }
& tar -xzf $Archive -C $Prefix
if ($LASTEXITCODE -ne 0) { throw 'Archive extraction failed' }
$installed = Join-Path $Prefix $name
& $Checker windows $installed $QualifiedDirectory $ExpectedSource $label $kind
if ($LASTEXITCODE -ne 0) { throw 'Installed archive admission failed' }
$version = ((Get-Content "$QualifiedDirectory/build-provenance.txt") | Where-Object { $_.StartsWith('binary_version=') }).Substring(15)
$oldPath = $env:PATH
try {
    $env:PATH = "$env:SystemRoot/System32"
    $observed = @(& "$installed/fastmash.exe" --version)
    if ($LASTEXITCODE -ne 0 -or $observed[0] -ne "fastmash $version") { throw 'Installed version differs' }
} finally { $env:PATH = $oldPath }
Write-Output $installed
