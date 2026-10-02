[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Set-Location $repositoryRoot

function Invoke-Cargo {
    param([string[]]$Arguments)

    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo failed with exit code $LASTEXITCODE"
    }
}

function Invoke-FormatCheck {
    param([string]$ManifestPath)

    Invoke-Cargo @("fmt", "--manifest-path", $ManifestPath, "--", "--check")
}

function Test-ProviderPackage {
    param([Parameter(Mandatory)] [string]$Path)

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $entries = @($archive.Entries | Where-Object { $_.Length -gt 0 })
        $names = @($entries | ForEach-Object { $_.FullName })
        $lowerNames = @($names | ForEach-Object { $_.ToLowerInvariant() })
        if (($lowerNames | Select-Object -Unique).Count -ne $lowerNames.Count) {
            throw "package contains duplicate paths: $Path"
        }
        if (-not ($names -contains "manifest.json")) {
            throw "package has no manifest.json: $Path"
        }
        $manifestEntry = $archive.GetEntry("manifest.json")
        $reader = [IO.StreamReader]::new($manifestEntry.Open())
        try {
            $manifest = $reader.ReadToEnd() | ConvertFrom-Json
        } finally {
            $reader.Dispose()
        }
        if ($manifest.manifestVersion -ne 2 -or $manifest.apiVersion -ne 3) {
            throw "package is not an API v3 component: $Path"
        }
        $componentPath = [string]$manifest.entrypoints.component
        if ([string]::IsNullOrWhiteSpace($componentPath) -or -not ($names -contains $componentPath)) {
            throw "package component entrypoint is missing: $Path"
        }
        if (-not ($manifest.permissions -contains "network:http://127.0.0.1:43821")) {
            throw "package does not declare the loopback backend origin: $Path"
        }
        $wasmEntries = @($entries | Where-Object { $_.FullName.ToLowerInvariant().EndsWith(".wasm") })
        if ($wasmEntries.Count -ne 1 -or $wasmEntries[0].FullName -ne $componentPath) {
            throw "package must contain exactly one component Wasm file: $Path"
        }
        $stream = $wasmEntries[0].Open()
        $binaryReader = [IO.BinaryReader]::new($stream)
        try {
            $magic = $binaryReader.ReadBytes(8)
        } finally {
            $binaryReader.Dispose()
            $stream.Dispose()
        }
        $expected = [byte[]](0, 97, 115, 109, 13, 0, 1, 0)
        $magicMatches = $magic.Length -eq $expected.Length
        if ($magicMatches) {
            for ($index = 0; $index -lt $expected.Length; $index++) {
                if ($magic[$index] -ne $expected[$index]) {
                    $magicMatches = $false
                    break
                }
            }
        }
        if (-not $magicMatches) {
            throw "component entrypoint is not a Wasm component: $Path"
        }
        $forbiddenExtensions = @(".exe", ".dll", ".so", ".dylib", ".node")
        foreach ($name in $names) {
            if ($name.StartsWith("/") -or $name.Contains("..\\") -or $name.Contains("../")) {
                throw "package contains an unsafe path: $Path"
            }
            if ($forbiddenExtensions | Where-Object { $name.ToLowerInvariant().EndsWith($_) }) {
                throw "package contains a native binary: $Path"
            }
        }
        Write-Output ("inspected {0}" -f $Path)
    } finally {
        $archive.Dispose()
    }
}

if (-not $SkipBuild) {
    Invoke-Cargo @("fmt", "--all", "--", "--check")
    Invoke-FormatCheck "plugins/guest-core/Cargo.toml"
    Invoke-FormatCheck "plugins/spotify/Cargo.toml"
    Invoke-FormatCheck "plugins/netease/Cargo.toml"
    Invoke-FormatCheck "plugins/kugou/Cargo.toml"
    Invoke-Cargo @("check", "--workspace", "--locked")
    Invoke-Cargo @("test", "--workspace", "--locked")
    Invoke-Cargo @("test", "--manifest-path", "plugins/guest-core/Cargo.toml", "--locked")
    Invoke-Cargo @("clippy", "--workspace", "--all-targets", "--locked", "--", "-D", "warnings")
    & (Join-Path $PSScriptRoot "build.ps1") -Mode isolated -OutputDirectory "dist/isolated"
    if ($LASTEXITCODE -ne 0) {
        throw "isolated package build failed"
    }
    & (Join-Path $PSScriptRoot "build.ps1") -Mode aggregate -OutputDirectory "dist/aggregate"
    if ($LASTEXITCODE -ne 0) {
        throw "aggregate package build failed"
    }
}

Get-ChildItem -LiteralPath (Join-Path $repositoryRoot "dist") -Recurse -Filter "*.yaqmc-plugin" |
    ForEach-Object { Test-ProviderPackage -Path $_.FullName }

Write-Output "validation complete"
