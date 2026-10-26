[CmdletBinding()]
param([string]$PackageDirectory = 'dist')
$ErrorActionPreference = 'Stop'
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$manifest = Get-Content -LiteralPath (Join-Path $repositoryRoot 'plugins/music/manifest.json') -Raw | ConvertFrom-Json
$fileName = '{0}-{1}.yaqmc-plugin' -f $manifest.id, $manifest.version
$packageRoot = if ([IO.Path]::IsPathRooted($PackageDirectory)) { $PackageDirectory } else { Join-Path $repositoryRoot $PackageDirectory }
$source = Join-Path $packageRoot $fileName
$validate = Join-Path $PSScriptRoot 'validate.ps1'
& $validate -SkipBuild -PackageDirectory $packageRoot
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('yaqmc-package-tests-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $testRoot | Out-Null
$owned = @()

function Write-Entry($Archive, [string]$Name, [byte[]]$Bytes) {
    $existing = $Archive.GetEntry($Name)
    if ($existing) { $existing.Delete() }
    $entry = $Archive.CreateEntry($Name)
    $stream = $entry.Open()
    try { $stream.Write($Bytes, 0, $Bytes.Length) } finally { $stream.Dispose() }
}

try {
    $cases = @(
        @{ Name = 'extra-file'; Mutate = { param($zip) Write-Entry $zip 'unexpected.txt' ([byte[]]@(1)) } },
        @{ Name = 'empty-traversal'; Mutate = { param($zip) Write-Entry $zip '../unexpected.exe' ([byte[]]@()) } },
        @{ Name = 'core-wasm'; Mutate = { param($zip) Write-Entry $zip 'component/provider.wasm' ([byte[]]@(0,97,115,109,1,0,0,0)) } },
        @{ Name = 'stale-permissions'; Mutate = {
            param($zip)
            $entry = $zip.GetEntry('manifest.json')
            $reader = [IO.StreamReader]::new($entry.Open())
            try { $changed = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
            $changed.permissions = @($changed.permissions | Where-Object { $_ -ne 'provider.comments' })
            Write-Entry $zip 'manifest.json' ([Text.Encoding]::UTF8.GetBytes(($changed | ConvertTo-Json -Depth 30)))
        } }
    )
    foreach ($case in $cases) {
        $directory = Join-Path $testRoot $case.Name
        New-Item -ItemType Directory -Path $directory | Out-Null
        $archivePath = Join-Path $directory $fileName
        $owned += @{ Directory = $directory; Archive = $archivePath }
        Copy-Item -LiteralPath $source -Destination $archivePath
        $zip = [IO.Compression.ZipFile]::Open($archivePath, [IO.Compression.ZipArchiveMode]::Update)
        try { & $case.Mutate $zip } finally { $zip.Dispose() }
        $rejected = $false
        try { & $validate -SkipBuild -PackageDirectory $directory } catch { $rejected = $true }
        if (-not $rejected) { throw "Invalid package was accepted: $($case.Name)" }
        Write-Output "rejected invalid package: $($case.Name)"
    }
} finally {
    # Only remove the exact files/directories created here; unexpected content
    # is preserved. No recursive cleanup or computed traversal is needed.
    foreach ($entry in $owned) {
        if (Test-Path -LiteralPath $entry.Archive) { Remove-Item -LiteralPath $entry.Archive }
        Remove-Item -LiteralPath $entry.Directory
    }
    Remove-Item -LiteralPath $testRoot
}
Write-Output 'packaging regression tests passed'
