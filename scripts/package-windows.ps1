param(
    [string]$BinaryDir = 'target/release',
    [string]$OutputDir = 'dist',
    [string]$IsccPath = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
    [string]$ReleaseTag = ''
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = Split-Path $PSScriptRoot -Parent
Push-Location $repoRoot
try {
    $metadataJson = & cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed' }
    $metadata = $metadataJson | ConvertFrom-Json
    $package = $metadata.packages | Where-Object name -EQ 'portweave'
    $version = $package.version
    if ($version -notmatch '^\d+\.\d+\.\d+$') {
        throw "Installer requires a stable numeric version, got: $version"
    }
    if ($ReleaseTag -and $ReleaseTag -cne "v$version") {
        throw "Release tag $ReleaseTag does not match Cargo version v$version"
    }
    if (-not (Test-Path -LiteralPath $IsccPath -PathType Leaf)) {
        throw "Inno Setup 6 compiler not found: $IsccPath"
    }
    $sourceDir = (Resolve-Path -LiteralPath $BinaryDir).Path
    if (-not (Test-Path -LiteralPath (Join-Path $sourceDir 'portweave.exe'))) {
        throw "Build portweave.exe in $sourceDir first"
    }
    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
    $destination = (Resolve-Path -LiteralPath $OutputDir).Path
    & $IsccPath "/DAppVersion=$version" "/DBinarySourceDir=$sourceDir" "/DOutputDir=$destination" 'installer/portweave.iss'
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compilation failed' }
    $installer = Join-Path $destination "PortWeave-$version-windows-x86_64-Setup.exe"
    if (-not (Test-Path -LiteralPath $installer)) { throw 'Installer output missing' }

    # A fresh staging directory prevents obsolete files entering the portable ZIP.
    $staging = Join-Path $destination ([guid]::NewGuid().ToString())
    $portable = Join-Path $staging 'PortWeave'
    try {
        New-Item -ItemType Directory -Path $portable | Out-Null
        Copy-Item -LiteralPath (Join-Path $sourceDir 'portweave.exe') -Destination $portable
        Copy-Item -LiteralPath 'README.md', 'LICENSE' -Destination $portable
        $archive = Join-Path $destination 'PortWeave-windows-x86_64.zip'
        Compress-Archive -LiteralPath $portable -DestinationPath $archive -Force
    } finally {
        # Only delete the freshly allocated staging directory inside the output path.
        $resolvedStaging = [IO.Path]::GetFullPath($staging)
        if ((Split-Path $resolvedStaging -Parent) -ne $destination) { throw 'Unsafe staging path' }
        Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
    }
    $hashes = foreach ($artifact in @($installer, $archive)) {
        $hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash *$(Split-Path $artifact -Leaf)"
    }
    $hashes | Set-Content (Join-Path $destination 'SHA256SUMS.txt') -Encoding utf8NoBOM
} finally {
    Pop-Location
}
