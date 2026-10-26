[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string]$Destination,
    [Parameter(Mandatory)] [string]$PortableExe,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $repositoryRoot
$destinationRoot = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $destinationRoot) { throw 'Use a new output directory; existing test bundles are never overwritten.' }
if (-not (Test-Path -LiteralPath $PortableExe -PathType Leaf)) { throw 'YAQMC portable executable is missing.' }
if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build.ps1') -OutputDirectory 'dist'
    if (-not $?) { throw 'Plugin build failed.' }
}
& (Join-Path $PSScriptRoot 'validate.ps1') -SkipBuild
if (-not $?) { throw 'Package validation failed.' }
New-Item -ItemType Directory -Path $destinationRoot | Out-Null
$packages = @(Get-ChildItem -LiteralPath (Join-Path $repositoryRoot 'dist') -Filter '*.yaqmc-plugin' -File)
if ($packages.Count -ne 1) { throw "Expected one provider package, found $($packages.Count)." }
Copy-Item -LiteralPath $packages[0].FullName -Destination $destinationRoot
foreach ($name in @('README.md', 'LICENSE', 'THIRD_PARTY_NOTICES.md')) {
    Copy-Item -LiteralPath (Join-Path $repositoryRoot $name) -Destination $destinationRoot
}
Copy-Item -LiteralPath (Join-Path $repositoryRoot 'docs') -Destination (Join-Path $destinationRoot 'docs') -Recurse
Copy-Item -LiteralPath $PortableExe -Destination (Join-Path $destinationRoot 'YAQMC-windows-x64-portable-provider-fix.exe')
$files = @(Get-ChildItem -LiteralPath $destinationRoot -File -Recurse -Force)
$destinationPrefix = $destinationRoot.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
$files | Sort-Object FullName | ForEach-Object {
    $fullName = [IO.Path]::GetFullPath($_.FullName)
    if (-not $fullName.StartsWith($destinationPrefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "bundle file is outside the destination root: $fullName"
    }
    $hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    '{0}  {1}' -f $hash, $fullName.Substring($destinationPrefix.Length).Replace('\', '/')
} | Set-Content -LiteralPath (Join-Path $destinationRoot 'SHA256SUMS.txt') -Encoding utf8
Write-Output "Manual test bundle: $destinationRoot"
