param(
    [Parameter(Mandatory)][string]$Root,
    [ValidateSet('Create', 'Remove')][string]$Action = 'Create'
)
$ErrorActionPreference = 'Stop'
$image = Join-Path $Root 'full.vhd'
$mount = Join-Path $Root 'mount'
if ($Action -eq 'Remove') {
    if (Test-Path $image) {
        if ((Get-DiskImage -ImagePath $image).Attached) { Dismount-DiskImage -ImagePath $image }
        if ((Get-DiskImage -ImagePath $image).Attached) { throw 'Private test image remains attached' }
        Remove-Item $image
    }
    if (Test-Path $mount) { Remove-Item $mount -Force }
    return
}
if ((Test-Path $image) -or (Test-Path $mount)) { throw 'Private volume paths must be fresh' }
New-Item -ItemType Directory -Path $mount -Force | Out-Null
@"
create vdisk file="$image" maximum=64 type=expandable
attach vdisk
create partition primary
format fs=ntfs quick label=FASTMASH_S2
assign mount="$mount"
exit
"@ | Set-Content (Join-Path $Root 'create-volume.txt') -Encoding ascii
& diskpart.exe /s (Join-Path $Root 'create-volume.txt') | Set-Content (Join-Path $Root 'volume-setup.txt')
if ($LASTEXITCODE -ne 0) { throw 'Private test volume creation failed' }
$volume = Get-Volume -FilePath ($mount + '\')
if ($volume.FileSystemLabel -ne 'FASTMASH_S2' -or $volume.Size -gt 80MB -or $volume.Size -lt 32MB) { throw 'Private test volume identity differs' }
$volume | Select-Object Path, FileSystemLabel, Size, SizeRemaining | ConvertTo-Json | Set-Content (Join-Path $Root 'volume.json')
# The native VM administrator can prepare the volume for its ordinary tester.
# Hosted checks run under their current user, whose SID receives the same access.
if ($env:FASTMASH_VOLUME_USER) {
    & icacls.exe $mount /grant ($env:FASTMASH_VOLUME_USER + ':(OI)(CI)F') | Set-Content (Join-Path $Root 'volume-acl.txt')
    if ($LASTEXITCODE -ne 0) { throw 'Private test volume permission setup failed' }
}
