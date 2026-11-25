param(
    [string]$Destination = (Join-Path $PSScriptRoot 'target\release\D3D12')
)

$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$runtimeName = 'D3D12Core.dll'
$runtimeHash = '70A6D47D335B4A15180544C01A35B84474522A369DC582D85FE057313A3AEAA0'
$runtime = Join-Path $Destination $runtimeName
$license = Join-Path $Destination 'LICENSE.txt'

if ((Test-Path -LiteralPath $runtime -PathType Leaf) -and
    (Test-Path -LiteralPath $license -PathType Leaf) -and
    (Get-FileHash -Algorithm SHA256 -LiteralPath $runtime).Hash -eq $runtimeHash) {
    return
}

$version = '1.620.2'
$package = "https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.d3d12/$version/microsoft.direct3d.d3d12.$version.nupkg"
$temporary = Join-Path ([IO.Path]::GetTempPath()) "victory-agility-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $archive = Join-Path $temporary 'agility.zip'
    curl.exe --fail --silent --show-error --location --max-time 90 --output $archive $package
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash -ne
        '70489669C2BBEAF318B62D72B0AC3E753D521387BFC848C9763791C4E920BFCC') {
        throw "Unexpected Agility SDK $version package checksum"
    }

    tar.exe -xf $archive -C $temporary "build/native/bin/x64/$runtimeName" LICENSE.txt
    $downloaded = Join-Path $temporary "build\native\bin\x64\$runtimeName"
    $signature = Get-AuthenticodeSignature -LiteralPath $downloaded
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'CN=Microsoft Corporation(?:,|$)' -or
        (Get-FileHash -Algorithm SHA256 -LiteralPath $downloaded).Hash -ne $runtimeHash) {
        throw "Agility SDK $version runtime authenticity check failed"
    }

    New-Item -ItemType Directory -Path $Destination -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $temporary 'LICENSE.txt') -Destination $license -Force
    Copy-Item -LiteralPath $downloaded -Destination $runtime -Force
    Write-Output "Staged DirectX Agility SDK $version in $Destination"
}
finally {
    Remove-Item -LiteralPath $temporary -Recurse -Force
}
