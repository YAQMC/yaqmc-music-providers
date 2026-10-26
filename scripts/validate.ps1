[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$PackageDirectory = "dist"
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

function Test-ProviderPackage {
    param([Parameter(Mandatory)] [string]$Path)

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $entries = @($archive.Entries)
        $names = @($entries | ForEach-Object { $_.FullName })
        if (($names | ForEach-Object { $_.ToLowerInvariant() } | Select-Object -Unique).Count -ne $names.Count) {
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
        if (@($manifest.providers).Count -ne 3) {
            throw "package must declare exactly three platform providers: $Path"
        }
        if ($manifest.permissions -contains "network:http://127.0.0.1:43821") {
            throw "package still grants the old loopback backend origin: $Path"
        }
        $componentPath = [string]$manifest.entrypoints.component
        if ($entries.Count -ne 2 -or $names -notcontains 'component/provider.wasm' -or $componentPath -ne 'component/provider.wasm') {
            throw "package must contain only its manifest and component: $Path"
        }
        if ($manifest.id -ne 'org.yaqmc.providers.music' -or $manifest.version -ne $expectedManifest.version) {
            throw "package identity does not match the current source manifest: $Path"
        }
        if (($manifest.providers | ConvertTo-Json -Depth 20 -Compress) -ne ($expectedManifest.providers | ConvertTo-Json -Depth 20 -Compress) -or
            ($manifest.permissions | ConvertTo-Json -Depth 20 -Compress) -ne ($expectedManifest.permissions | ConvertTo-Json -Depth 20 -Compress)) {
            throw "package capabilities or permissions do not match the source: $Path"
        }
        if ([string]::IsNullOrWhiteSpace($componentPath) -or -not ($names -contains $componentPath)) {
            throw "package component entrypoint is missing: $Path"
        }
        $wasmEntries = @($entries | Where-Object { $_.FullName.ToLowerInvariant().EndsWith(".wasm") })
        if ($wasmEntries.Count -ne 1 -or $wasmEntries[0].FullName -ne $componentPath) {
            throw "package must contain exactly one component Wasm file: $Path"
        }
        $componentStream = $wasmEntries[0].Open()
        try {
            $memory = [IO.MemoryStream]::new()
            $componentStream.CopyTo($memory)
            $componentBytes = $memory.ToArray()
        } finally {
            $componentStream.Dispose()
        }
        if ($componentBytes.Length -lt 8 -or
            $componentBytes[0] -ne 0 -or $componentBytes[1] -ne 0x61 -or
            $componentBytes[2] -ne 0x73 -or $componentBytes[3] -ne 0x6d -or
            $componentBytes[4] -ne 0x0d -or $componentBytes[5] -ne 0 -or
            $componentBytes[6] -ne 1 -or $componentBytes[7] -ne 0) {
            throw "package entrypoint is not a WebAssembly Component: $Path"
        }
        if ([Text.Encoding]::ASCII.GetString($componentBytes).Contains("wasi:")) {
            throw "package component unexpectedly imports WASI: $Path"
        }
        foreach ($name in $names) {
            if ($name.StartsWith("/") -or $name.Contains("..\") -or $name.Contains("../")) {
                throw "package contains an unsafe path: $Path"
            }
            if ($name -match "(?i)(\.exe|\.dll|\.so|\.dylib|\.node)$" -or $name -match "(?i)(node_modules|backend|runtime)") {
                throw "package contains a legacy runtime artifact: $Path"
            }
        }
        Write-Output ("inspected {0}" -f $Path)
    } finally {
        $archive.Dispose()
    }
}

if (-not $SkipBuild) {
    Invoke-Cargo @("fmt", "--manifest-path", "plugins/music/Cargo.toml", "--", "--check")
    Invoke-Cargo @(
        "check",
        "--locked",
        "--target",
        "wasm32-unknown-unknown",
        "--manifest-path",
        "plugins/music/Cargo.toml"
    )
    Invoke-Cargo @("test", "--locked", "--manifest-path", "plugins/music/Cargo.toml")
    & (Join-Path $PSScriptRoot "build.ps1") -OutputDirectory $PackageDirectory
    if ($LASTEXITCODE -ne 0) {
        throw "plugin package build failed"
    }
}

$expectedManifest = Get-Content -LiteralPath (Join-Path $repositoryRoot "plugins/music/manifest.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$packageRoot = if ([IO.Path]::IsPathRooted($PackageDirectory)) { [IO.Path]::GetFullPath($PackageDirectory) } else { Join-Path $repositoryRoot $PackageDirectory }
$packagePath = Join-Path $packageRoot ("{0}-{1}.yaqmc-plugin" -f $expectedManifest.id, $expectedManifest.version)
if (-not (Test-Path -LiteralPath $packagePath -PathType Leaf)) { throw "current plugin package is missing: $packagePath" }
Test-ProviderPackage -Path $packagePath
Write-Output "validation complete"
