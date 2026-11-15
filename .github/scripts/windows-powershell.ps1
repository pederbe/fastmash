param([Parameter(Mandatory)][string]$Root, [string]$EnvironmentFile = $env:GITHUB_ENV)
$ErrorActionPreference = 'Stop'
if (-not $EnvironmentFile) { throw 'Pass an environment output file' }
$versions = @(
    @{ Version = '7.4.20'; Hash = 'fb88cd3731847006b157978d6c0a426390d346696d91d1ed85b5e5eb88cb2d40'; Variable = 'FASTMASH_PWSH_74' },
    @{ Version = '7.7.0'; Hash = '7c74e03b20a75b59715847439d046a6214b1863ee79b187872980f139bb4a05f'; Variable = 'FASTMASH_PWSH_CURRENT' }
)
New-Item -ItemType Directory -Path $Root -Force | Out-Null
foreach ($entry in $versions) {
    $version = $entry.Version
    $archive = Join-Path $Root "PowerShell-$version-win-x64.zip"
    $directory = Join-Path $Root "pwsh-$version"
    if (-not (Test-Path "$directory\pwsh.exe")) {
        Invoke-WebRequest "https://github.com/PowerShell/PowerShell/releases/download/v$version/PowerShell-$version-win-x64.zip" -OutFile $archive
        if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Hash) { throw "PowerShell $version archive hash differs" }
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
        Expand-Archive -LiteralPath $archive -DestinationPath $directory
        Remove-Item $archive
    }
    "$($entry.Variable)=$directory\pwsh.exe" | Out-File -FilePath $EnvironmentFile -Append -Encoding utf8
}
