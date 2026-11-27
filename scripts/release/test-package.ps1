# Dry run of package.ps1 on a fake engine folder. Exit code 0 when the output is as documented.
$ErrorActionPreference = 'Stop'
$tmp = Join-Path ([IO.Path]::GetTempPath()) "vu-pkgtest-$([guid]::NewGuid().ToString('N'))"
$eng = Join-Path $tmp 'engine'; New-Item -ItemType Directory -Force "$eng\D3D12" | Out-Null
Set-Content "$eng\victory.exe" 'MZ'; Set-Content "$eng\D3D12\D3D12Core.dll" 'x'; Set-Content "$eng\victory.pdb" 'sym'
& (Join-Path $PSScriptRoot 'package.ps1') -EngineDir $eng -DryRun -Build 'abcdef0123456789abcdef0123456789abcdef01' -Out "$tmp\dist" -BaseUrl 'https://example.invalid/d' | Out-Null
$m = Get-Content "$tmp\dist\manifest.json" -Raw | ConvertFrom-Json
$e = $m.channels.stable.engines[0]
if ($m.schema -ne 1 -or $e.kind -ne 'release' -or $e.build -notlike 'abcdef01*') { throw 'manifest content is wrong' }
if ((Get-FileHash "$tmp\dist\engine-abcdef01-release.zip" -Algorithm SHA256).Hash.ToLower() -ne $e.sha256) { throw 'sha256 mismatch' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
function Get-ZipNames($path) { $z = [IO.Compression.ZipFile]::OpenRead($path); try { $z.Entries.FullName } finally { $z.Dispose() } }
$names = Get-ZipNames "$tmp\dist\engine-abcdef01-release.zip"
if ($names -notcontains 'victory.exe') { throw 'victory.exe missing in engine zip' }
if ($names -contains 'victory.pdb') { throw 'pdb must not be packaged' }
$full = Get-ZipNames (Get-ChildItem "$tmp\dist\victory-united-*.zip").FullName
if (-not ($full -match 'current\.txt')) { throw 'current.txt missing' }
# A DLSS file must make the script refuse.
Set-Content "$eng\nvngx_dlss.dll" 'x'
$refused = $false; try { & (Join-Path $PSScriptRoot 'package.ps1') -EngineDir $eng -DryRun -Out "$tmp\dist2" | Out-Null } catch { $refused = $true }
if (-not $refused) { throw 'nvngx file was not refused' }
Remove-Item -Recurse -Force $tmp
Write-Host 'package.ps1 dry run OK'
