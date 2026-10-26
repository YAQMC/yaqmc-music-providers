[CmdletBinding()]
param(
    [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$outputRoot = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
    [IO.Path]::GetFullPath($OutputDirectory)
} else {
    [IO.Path]::GetFullPath((Join-Path $repositoryRoot $OutputDirectory))
}
$target = "wasm32-unknown-unknown"
$manifestRelative = "plugins/music/manifest.json"
$artifactName = "yaqmc_provider_music.wasm"

function Invoke-Cargo {
    param([string[]]$Arguments)

    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo failed with exit code $LASTEXITCODE"
    }
}

function New-PluginArchive {
    param(
        [Parameter(Mandatory)] [string]$Source,
        [Parameter(Mandatory)] [string]$Destination
    )

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    if (Test-Path -LiteralPath $Destination) {
        Remove-Item -LiteralPath $Destination -Force
    }

    $sourceRoot = [IO.Path]::GetFullPath($Source).TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    $stream = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite)
    $archive = [IO.Compression.ZipArchive]::new(
        $stream,
        [IO.Compression.ZipArchiveMode]::Create,
        $false
    )
    try {
        @('manifest.json', 'component/provider.wasm') |
            ForEach-Object {
                $fullName = [IO.Path]::GetFullPath((Join-Path $Source $_))
                if (-not $fullName.StartsWith($sourceRoot, [StringComparison]::OrdinalIgnoreCase)) {
                    throw "archive input is outside the package root: $fullName"
                }
                $relative = [string]$_
                $entry = $archive.CreateEntry($relative, [IO.Compression.CompressionLevel]::Optimal)
                $entry.LastWriteTime = [DateTimeOffset]::new(1980, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
                $input = [IO.File]::OpenRead($fullName)
                $output = $entry.Open()
                try {
                    $input.CopyTo($output)
                } finally {
                    $output.Dispose()
                    $input.Dispose()
                }
            }
    } finally {
        $archive.Dispose()
        $stream.Dispose()
    }
}

$manifestPath = Join-Path $repositoryRoot $manifestRelative
$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$packageRoot = Join-Path $repositoryRoot "artifacts/music"
$componentRoot = Join-Path $packageRoot "component"
$artifactPath = Join-Path $repositoryRoot "plugins/music/target/$target/release/$artifactName"
$componentPath = Join-Path $componentRoot "provider.wasm"

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
New-Item -ItemType Directory -Path $componentRoot -Force | Out-Null

Invoke-Cargo @(
    "build",
    "--locked",
    "--release",
    "--target",
    $target,
    "--manifest-path",
    (Join-Path $repositoryRoot "plugins/music/Cargo.toml")
)

if (-not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
    throw "Wasm artifact was not produced: $artifactPath"
}
Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $packageRoot "manifest.json")
Invoke-Cargo @(
    "run",
    "--locked",
    "--release",
    "--manifest-path",
    (Join-Path $repositoryRoot "Cargo.toml"),
    "-p",
    "yaqmc-componentize",
    "--",
    $artifactPath,
    $componentPath
)

$packagePath = Join-Path $outputRoot ("{0}-{1}.yaqmc-plugin" -f $manifest.id, $manifest.version)
New-PluginArchive -Source $packageRoot -Destination $packagePath
Write-Output ("built {0}" -f $packagePath)
