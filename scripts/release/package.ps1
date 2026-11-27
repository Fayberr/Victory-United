<#
.SYNOPSIS
  Builds the release files from a folder that holds a built victory.exe.

.DESCRIPTION
  Produces, in -Out:
    victory-united-<version>-windows-x64.zip    launcher, engine\ (current build) and notices
    engine-<build>-<kind>.zip                   the engine alone, what the launcher's updater downloads
    manifest.json                               release manifest (schema 1), see docs/release.md
    SHA256SUMS.txt                              SHA-256 of every file above
  Nothing is uploaded and no tag is created.

.PARAMETER EngineDir
  Folder with victory.exe and what the engine needs next to it (D3D12 folder, DLLs).
.PARAMETER LauncherExe
  Path of victory-launcher.exe. Optional with -DryRun.
.PARAMETER Kind
  release or diagnostics.
.PARAMETER Build
  Commit the engine was built from (default: git rev-parse HEAD).
.PARAMETER Channel
  stable or dev. Decides the channel the manifest lists the engine under.
.PARAMETER BaseUrl
  Where the files will be downloadable, e.g. https://github.com/Fayberr/Victory-United/releases/download/v0.1.0
.PARAMETER DryRun
  Works on any folder (a fake victory.exe is fine) and does not need a launcher exe.
#>
param(
    [Parameter(Mandatory)][string]$EngineDir,
    [string]$LauncherExe = '',
    [ValidateSet('release', 'diagnostics')][string]$Kind = 'release',
    [string]$Build = '',
    [ValidateSet('stable', 'dev')][string]$Channel = 'stable',
    [string]$Version = '0.1.0',
    [string]$Translator = '',
    [string]$Notes = '',
    [string]$BaseUrl = 'https://github.com/Fayberr/Victory-United/releases/download/TAG',
    [string]$Out = 'dist',
    [switch]$DryRun
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)

if (-not (Test-Path (Join-Path $EngineDir 'victory.exe'))) { throw "victory.exe is not in $EngineDir" }
if (-not $DryRun -and -not (Test-Path $LauncherExe)) { throw "launcher exe not found: $LauncherExe (use -DryRun to skip it)" }
if (-not $Build) { $Build = (git -C $root rev-parse HEAD 2>$null); if (-not $Build) { $Build = '0' * 40 } }
$short = ($Build -replace '[^0-9A-Za-z]', '').Substring(0, [Math]::Min(8, ($Build -replace '[^0-9A-Za-z]', '').Length))
$folder = "$short-$Kind"

# Files that must never go into a package (see docs/release.md).
$banned = @('nvngx*.dll', 'sl.*.dll', 'nvngx_dlss*.dll', 'MicrosoftGame.Config', '*.xvc', '*.xvd')
foreach ($pattern in $banned) {
    $hit = Get-ChildItem -Path $EngineDir -Recurse -Filter $pattern -File -ErrorAction SilentlyContinue
    if ($hit) { throw "refusing to package $($hit[0].FullName): $pattern is not redistributable here" }
}

New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$stage = Join-Path ([IO.Path]::GetTempPath()) "vu-package-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force $stage | Out-Null
try {
    # 1. Engine ZIP: the engine folder content at the top level, without .pdb files.
    $engineStage = Join-Path $stage 'engine-content'
    New-Item -ItemType Directory -Force $engineStage | Out-Null
    Get-ChildItem -Path $EngineDir -Force | Where-Object { $_.Extension -ne '.pdb' } |
        Copy-Item -Destination $engineStage -Recurse -Force
    Get-ChildItem -Path $engineStage -Recurse -Filter '*.pdb' -ErrorAction SilentlyContinue | Remove-Item -Force
    $engineZip = Join-Path $Out "engine-$short-$Kind.zip"
    if (Test-Path $engineZip) { Remove-Item $engineZip }
    Compress-Archive -Path (Join-Path $engineStage '*') -DestinationPath $engineZip -CompressionLevel Optimal

    # 2. Full ZIP: launcher exe, engine\<folder>, engine\current.txt, notices.
    $full = Join-Path $stage 'full'
    New-Item -ItemType Directory -Force (Join-Path $full "engine\$folder") | Out-Null
    Copy-Item -Path (Join-Path $engineStage '*') -Destination (Join-Path $full "engine\$folder") -Recurse -Force
    Set-Content -Path (Join-Path $full 'engine\current.txt') -Value $folder -NoNewline -Encoding ascii
    if ($LauncherExe -and (Test-Path $LauncherExe)) { Copy-Item $LauncherExe (Join-Path $full 'victory-launcher.exe') }
    foreach ($n in 'LICENSE', 'README.md') {
        if (Test-Path (Join-Path $root $n)) { Copy-Item (Join-Path $root $n) $full }
    }
    foreach ($n in 'docs\launcher.md', 'docs\credits.md', 'docs\asset-policy.md') {
        if (Test-Path (Join-Path $root $n)) { Copy-Item (Join-Path $root $n) $full }
    }
    $fullZip = Join-Path $Out "victory-united-$Version-windows-x64.zip"
    if (Test-Path $fullZip) { Remove-Item $fullZip }

    # 3. Manifest. The engine entry points at the engine ZIP.
    $engineHash = (Get-FileHash $engineZip -Algorithm SHA256).Hash.ToLower()
    $engineEntry = [ordered]@{
        build      = $Build
        translator = $Translator
        kind       = $Kind
        url        = "$BaseUrl/$(Split-Path -Leaf $engineZip)"
        sha256     = $engineHash
        size       = (Get-Item $engineZip).Length
        notes      = $Notes
    }
    $manifest = [ordered]@{
        schema           = 1
        launcher_version = $Version
        launcher_url     = ($BaseUrl -replace '/download/', '/tag/')
        channels         = [ordered]@{ $Channel = [ordered]@{ engines = @($engineEntry) } }
    }
    $manifestPath = Join-Path $Out 'manifest.json'
    $json = $manifest | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText($manifestPath, $json, (New-Object Text.UTF8Encoding($false)))
    Copy-Item $manifestPath (Join-Path $full 'manifest.json') -Force
    Compress-Archive -Path (Join-Path $full '*') -DestinationPath $fullZip -CompressionLevel Optimal

    # 4. Checksums.
    $sums = foreach ($f in $fullZip, $engineZip, $manifestPath) {
        '{0}  {1}' -f (Get-FileHash $f -Algorithm SHA256).Hash.ToLower(), (Split-Path -Leaf $f)
    }
    Set-Content -Path (Join-Path $Out 'SHA256SUMS.txt') -Value $sums -Encoding ascii
    Write-Host "Packaged $folder into $Out"
    $sums | ForEach-Object { Write-Host $_ }
}
finally {
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
}
