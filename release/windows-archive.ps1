param(
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$WorkDirectory,
    [ValidateSet('development', 'release')][string]$Mode = 'development',
    [ValidateSet('all', 'first', 'second', 'package')][string]$Phase = 'all'
)
# Build glue. Admission and byte comparisons live in the Rust archive checker.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$target = 'x86_64-pc-windows-msvc'
function Native([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed: $LASTEXITCODE" }
}
Push-Location $root
$names = @('CARGO_TARGET_DIR','CARGO_ENCODED_RUSTFLAGS','RUSTFLAGS','CARGO_INCREMENTAL','SOURCE_DATE_EPOCH','CARGO_BUILD_TARGET','LIB','INCLUDE','PATH')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name) }
try {
    if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') { throw 'Build on native Windows x64 with PowerShell 7.4+' }
    if ($PSVersionTable.PSVersion -lt [version]'7.4') { throw 'PowerShell 7.4+ is required' }
    if ((& rustc -Vv) -notcontains "host: x86_64-pc-windows-msvc") { throw "Use the MSVC Rust host toolchain" }
    $dirty = & git status --porcelain
    if ($LASTEXITCODE -ne 0 -or $dirty) { throw 'Build from a clean source checkout' }
    $source = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Source identity unavailable' }
    $version = (Select-String '^version = "([^"]+)"' "$root/Cargo.toml").Matches.Groups[1].Value
    $work = [IO.Path]::GetFullPath($WorkDirectory)
    $out = [IO.Path]::GetFullPath($OutputDirectory)
    if ($Phase -in @('all','first')) {
        if (Test-Path $work) { throw 'Use a fresh build work directory' }
        New-Item -ItemType Directory $work | Out-Null
        "$source`n$Mode" | Set-Content "$work/source.txt"
    }
    if ((Get-Content "$work/source.txt" -Raw).Trim() -ne "$source`n$Mode") { throw 'Source or archive mode changed between phases' }
    $env:CARGO_INCREMENTAL = '0'
    $env:SOURCE_DATE_EPOCH = (& git show -s --format=%ct HEAD).Trim()
    Remove-Item Env:RUSTFLAGS,Env:CARGO_ENCODED_RUSTFLAGS,Env:CARGO_BUILD_TARGET -ErrorAction SilentlyContinue
    if ($Phase -in @('all','first')) {
        $env:CARGO_TARGET_DIR = "$work/tools"
        Native cargo @('build','--locked','--manifest-path','release/publication-test-server/Cargo.toml','--bin','fastmash-check-archive')
    }
    $checker = "$work/tools/debug/fastmash-check-archive.exe"
    $vswhere = "${env:ProgramFiles(x86)}/Microsoft Visual Studio/Installer/vswhere.exe"
    $vs = (& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath).Trim()
    $vc = (Get-Content "$vs/VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt").Trim()
    $vcRoot = "$vs/VC/Tools/MSVC/$vc"
    $sdkRoot = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots','HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows Kits\Installed Roots' -ErrorAction SilentlyContinue | Where-Object { $_.KitsRoot10 } | Select-Object -First 1).KitsRoot10
    $sdk = (Get-ChildItem "$sdkRoot/Lib" -Directory | Where-Object { (Test-Path "$($_.FullName)/um/x64/kernel32.lib") -and (Test-Path "$($_.FullName)/ucrt/x64/libucrt.lib") } | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1).Name
    if (-not $sdk) { throw 'Windows SDK libraries unavailable' }
    $env:LIB = "$vcRoot/lib/x64;$sdkRoot/Lib/$sdk/um/x64;$sdkRoot/Lib/$sdk/ucrt/x64"
    $env:INCLUDE = "$vcRoot/include;$sdkRoot/Include/$sdk/ucrt;$sdkRoot/Include/$sdk/shared;$sdkRoot/Include/$sdk/um;$sdkRoot/Include/$sdk/winrt"
    $env:PATH = "$vcRoot/bin/Hostx64/x64;$sdkRoot/bin/$sdk/x64;$env:PATH"
    $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $flags = @('-C','target-cpu=x86-64','-C','target-feature=+crt-static','-C','link-arg=/Brepro','-C',"linker=$vcRoot/bin/Hostx64/x64/link.exe",
        "--remap-path-prefix=$root=/fastmash", "--remap-path-prefix=$cargoHome/registry/src=/deps",
        "--remap-path-prefix=$work/first=/target", "--remap-path-prefix=$work/second=/target")
    $env:CARGO_ENCODED_RUSTFLAGS = $flags -join [char]31
    foreach ($build in @('first','second')) {
        if ($build -eq 'second' -and $Mode -eq 'development') { continue }
        if ($Phase -ne 'all' -and $Phase -ne $build) { continue }
        $directory = "$work/$build"
        if (Test-Path $directory) { throw 'Optimized builds require empty target directories' }
        $env:CARGO_TARGET_DIR = $directory
        Native cargo @('build','--locked','--release','-p','fastmash','--bin','fastmash','--target',$target)
        @($source,$vc,$sdk,$($flags -join ' '),$(& rustc -Vv)) | Set-Content "$work/config-$build.txt"
    }
    if ($Phase -notin @('all','package')) { return }
    $binary = "$work/first/$target/release/fastmash.exe"
    if ($Mode -eq 'release') {
        if (Compare-Object (Get-Content "$work/config-first.txt") (Get-Content "$work/config-second.txt")) { throw 'Compiler or build inputs changed between clean builds' }
        & $checker bytes $binary "$work/second/$target/release/fastmash.exe"
        if ($LASTEXITCODE -ne 0) {
            New-Item -ItemType Directory $out -Force | Out-Null
            Copy-Item $binary "$out/failed-first.exe"
            Copy-Item "$work/second/$target/release/fastmash.exe" "$out/failed-second.exe"
            Copy-Item "$work/config-*.txt" $out
            throw 'Clean Windows executable builds differ'
        }
    }
    if (Test-Path $out) { throw 'Use a fresh archive output directory' }
    New-Item -ItemType Directory $out | Out-Null
    $label = if ($Mode -eq 'release') { $version } else { "$version-windows-test-$($source.Substring(0,12))" }
    $kind = if ($Mode -eq 'release') { 'release-candidate' } else { 'development-test' }
    $name = "fastmash-v$label-$target"
    $stage = "$work/$name"
    New-Item -ItemType Directory $stage | Out-Null
    Copy-Item $binary "$stage/fastmash.exe"
    Native $checker @('bytes',$binary,"$stage/fastmash.exe")
    $dumpbin = "$vcRoot/bin/Hostx64/x64/dumpbin.exe"
    & $dumpbin /HEADERS $binary | Set-Content "$out/pe-headers.txt"
    if ($LASTEXITCODE -ne 0) { throw 'PE header inspection failed' }
    & $dumpbin /IMPORTS $binary | Set-Content "$out/pe-imports.txt"
    if ($LASTEXITCODE -ne 0) { throw 'PE dependency inspection failed' }
    Native $checker @('pe',"$out/pe-headers.txt","$out/pe-imports.txt")
    foreach ($document in @('README.md','CHANGELOG.md','LICENSE-MIT','LICENSE-APACHE')) { Copy-Item "$root/$document" $out }
    $env:CARGO_TARGET_DIR = "$work/tools"
    Remove-Item Env:CARGO_ENCODED_RUSTFLAGS
    & cargo run --locked --release -p fastmash-license-notices -- --target $target > "$out/THIRD-PARTY-LICENSES.md"
    if ($LASTEXITCODE -ne 0) { throw 'Windows notices failed' }
    $hash = (Get-FileHash $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  fastmash.exe" | Set-Content "$out/binary.sha256" -Encoding ascii
    $builds = if ($Mode -eq 'release') { 2 } else { 1 }
    @("artifact_kind=$kind","archive_label=$label","binary_version=$version","clean_builds=$builds",
      "source_revision=$source","target=$target","CARGO_INCREMENTAL=0","rustflags_json=$($flags | ConvertTo-Json -Compress)",
      "MSVC=$vc","WindowsSDK=$sdk",'',$(& rustc -Vv),$(& cargo -V),$(& cmd /c ver)) | Set-Content "$out/build-provenance.txt"
    foreach ($document in @('README.md','CHANGELOG.md','LICENSE-MIT','LICENSE-APACHE','THIRD-PARTY-LICENSES.md','binary.sha256','build-provenance.txt')) { Copy-Item "$out/$document" $stage }
    Native $checker @('windows',$stage,$out,$source,$label,$kind)
    Native tar @('-czf',"$out/$name.tar.gz",'-C',$work,$name)
    "$((Get-FileHash "$out/$name.tar.gz" -Algorithm SHA256).Hash.ToLowerInvariant())  $name.tar.gz" | Set-Content "$out/$name.tar.gz.sha256" -Encoding ascii
    $label | Set-Content "$out/archive-version.txt"
    $kind | Set-Content "$out/archive-kind.txt"
    Write-Output "$out/$name.tar.gz"
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name,$saved[$name]) }
    Pop-Location
}
