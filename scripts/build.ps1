[CmdletBinding()]
param(
    [ValidateSet("isolated", "aggregate")]
    [string]$Mode = "isolated",
    [string]$BackendUrl = "http://127.0.0.1:43821/v1",
    [string]$OutputDirectory = "dist"
)

$ErrorActionPreference = "Stop"

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$outputRoot = if ([IO.Path]::IsPathRooted($OutputDirectory)) {
    [IO.Path]::GetFullPath($OutputDirectory)
} else {
    [IO.Path]::GetFullPath((Join-Path $repositoryRoot $OutputDirectory))
}
$artifactRoot = Join-Path $repositoryRoot (Join-Path "artifacts" $Mode)
$target = "wasm32-wasip2"

if ($BackendUrl -ne "http://127.0.0.1:43821/v1") {
    throw "BackendUrl must remain http://127.0.0.1:43821/v1 so the package matches YAQMC's loopback grant."
}

$plugins = @(
    [ordered]@{
        Name = "spotify"
        Manifest = "plugins/spotify/manifest.json"
        Artifact = "yaqmc_provider_spotify.wasm"
    },
    [ordered]@{
        Name = "netease"
        Manifest = "plugins/netease/manifest.json"
        Artifact = "yaqmc_provider_netease.wasm"
    },
    [ordered]@{
        Name = "kugou"
        Manifest = "plugins/kugou/manifest.json"
        Artifact = "yaqmc_provider_kugou.wasm"
    }
)

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

    $stream = [IO.File]::Open($Destination, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite)
    $archive = [IO.Compression.ZipArchive]::new(
        $stream,
        [IO.Compression.ZipArchiveMode]::Create,
        $false
    )
    try {
        Get-ChildItem -LiteralPath $Source -Recurse -File |
            Sort-Object FullName |
            ForEach-Object {
                $relative = [IO.Path]::GetRelativePath($Source, $_.FullName).Replace('\', '/')
                $entry = $archive.CreateEntry($relative, [IO.Compression.CompressionLevel]::Optimal)
                $input = [IO.File]::OpenRead($_.FullName)
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

New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null
New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null
$env:YAQMC_PROVIDER_MODE = $Mode
$env:YAQMC_PROVIDER_BACKEND_URL = $BackendUrl

foreach ($plugin in $plugins) {
    $manifestPath = Join-Path $repositoryRoot $plugin.Manifest
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $packageRoot = Join-Path $artifactRoot $plugin.Name
    $componentRoot = Join-Path $packageRoot "component"
    if (Test-Path -LiteralPath $packageRoot) {
        Remove-Item -LiteralPath $packageRoot -Recurse -Force
    }
    New-Item -ItemType Directory -Path $componentRoot -Force | Out-Null

    Invoke-Cargo @(
        "build",
        "--locked",
        "--release",
        "--target",
        $target,
        "--manifest-path",
        (Join-Path $repositoryRoot ("plugins/{0}/Cargo.toml" -f $plugin.Name))
    )

    $artifactPath = Join-Path $repositoryRoot ("plugins/{0}/target/{1}/release/{2}" -f $plugin.Name, $target, $plugin.Artifact)
    if (-not (Test-Path -LiteralPath $artifactPath -PathType Leaf)) {
        throw "Wasm artifact was not produced: $artifactPath"
    }
    Copy-Item -LiteralPath $manifestPath -Destination (Join-Path $packageRoot "manifest.json")
    Copy-Item -LiteralPath $artifactPath -Destination (Join-Path $componentRoot "provider.wasm")

    $packagePath = Join-Path $outputRoot ("{0}-{1}-{2}.yaqmc-plugin" -f $manifest.id, $manifest.version, $Mode)
    New-PluginArchive -Source $packageRoot -Destination $packagePath
    Write-Output ("built {0}" -f $packagePath)
}

if ($Mode -eq "aggregate") {
    Write-Warning "Install only one aggregate package; each package fans out the same external platforms."
}
