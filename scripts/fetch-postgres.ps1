<#
.SYNOPSIS
    Downloads the pinned Postgres 17 Windows x64 binaries for the standalone desktop app.

.DESCRIPTION
    Reads the archive URL and SHA-256 from desktop/postgres-pin.txt, downloads the zip (or uses
    -ZipPath), refuses it unless the SHA-256 matches, then extracts only bin/, lib/, share/ and the
    server licence into -Destination. Nothing is installed, no service is created, PATH is untouched.

    The EDB binaries need the Visual C++ runtime (vcruntime140.dll, vcruntime140_1.dll, msvcp140.dll), which a clean
    Windows install does not have and a per-user installer cannot install. They are copied into bin/ from the Visual
    Studio redistributable folder (app-local deployment, which Microsoft allows for these files), then
    scripts/check-pe-imports.ps1 checks that every .exe/.dll of bin/ and lib/ finds its VC++ runtime inside the
    folder. The script fails when the redistributable folder or one of the files is missing.

.PARAMETER Destination
    Output folder (default: target/postgres in the repository). Replaced if it exists.

.PARAMETER ZipPath
    Use an already downloaded archive instead of downloading. It is still hash-checked.

.PARAMETER VcRedistDir
    Folder holding the x64 VC++ runtime DLLs (Microsoft.VC14x.CRT). Default: $env:VCToolsRedistDir\x64\Microsoft.VC14*.CRT
    when set (a Developer prompt), otherwise the newest VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT of the Visual Studio
    installation vswhere reports.

.EXAMPLE
    pwsh scripts/fetch-postgres.ps1
    $env:PG_BIN_DIR = "$PWD\target\postgres\bin"; cargo test -p storm-codex-supervisor --test postgres_it
#>
[CmdletBinding()]
param(
    [string]$Destination,
    [string]$ZipPath,
    [string]$VcRedistDir
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Destination) { $Destination = Join-Path $repoRoot 'target\postgres' }
$pinFile = Join-Path $repoRoot 'desktop\postgres-pin.txt'

$pin = @{}
foreach ($line in Get-Content -LiteralPath $pinFile) {
    $line = $line.Trim()
    if ($line -eq '' -or $line.StartsWith('#')) { continue }
    $key, $value = $line -split '=', 2
    $pin[$key.Trim()] = $value.Trim()
}
foreach ($key in 'url', 'sha256') {
    if (-not $pin.ContainsKey($key) -or -not $pin[$key]) { throw "postgres-pin.txt: missing '$key'" }
}

# VC++ runtime DLLs the EDB binaries import (scripts/check-pe-imports.ps1 lists them).
$vcRuntimeDlls = @('vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll')

function Find-VcRedistDir {
    $candidates = @()
    if ($env:VCToolsRedistDir) {
        $x64 = Join-Path $env:VCToolsRedistDir 'x64'
        if (Test-Path -LiteralPath $x64) {
            $candidates += Get-ChildItem -LiteralPath $x64 -Directory -Filter 'Microsoft.VC14*.CRT'
        }
    }
    if (-not $candidates) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
        if (Test-Path -LiteralPath $vswhere) {
            foreach ($vs in & $vswhere -all -products * -property installationPath) {
                $msvc = Join-Path $vs 'VC\Redist\MSVC'
                if (-not (Test-Path -LiteralPath $msvc)) { continue }
                foreach ($ver in Get-ChildItem -LiteralPath $msvc -Directory | Where-Object { $_.Name -match '^\d+\.\d+' }) {
                    $x64 = Join-Path $ver.FullName 'x64'
                    if (Test-Path -LiteralPath $x64) {
                        $candidates += Get-ChildItem -LiteralPath $x64 -Directory -Filter 'Microsoft.VC14*.CRT'
                    }
                }
            }
        }
    }
    # newest MSVC version folder (…\MSVC\<version>\x64\Microsoft.VC14x.CRT)
    $best = $candidates | Sort-Object { [version](($_.Parent.Parent.Name -split '[^\d.]')[0]) } -Descending | Select-Object -First 1
    if (-not $best) {
        throw "Visual C++ redistributable folder not found (Microsoft.VC14*.CRT): install the VC++ build tools or pass -VcRedistDir"
    }
    return $best.FullName
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("storm-codex-pg-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    if ($ZipPath) {
        $zip = (Resolve-Path -LiteralPath $ZipPath).Path
    } else {
        $zip = Join-Path $work 'postgres.zip'
        Write-Host "Downloading $($pin.url)"
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -Uri $pin.url -OutFile $zip -UseBasicParsing
    }

    $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $pin.sha256.ToLowerInvariant()) {
        throw "SHA-256 mismatch for $zip`n  expected $($pin.sha256)`n  actual   $actual"
    }
    Write-Host "SHA-256 verified: $actual"

    if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
    New-Item -ItemType Directory -Path $Destination | Out-Null
    $destRoot = [IO.Path]::GetFullPath($Destination).TrimEnd('\') + '\'

    # The EDB archive has a single top-level `pgsql/` folder.
    $keepDirs = @('pgsql/bin/', 'pgsql/lib/', 'pgsql/share/')
    $keepFiles = @('pgsql/server_license.txt')

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($zip)
    $count = 0
    try {
        foreach ($entry in $archive.Entries) {
            $name = $entry.FullName.Replace('\', '/')
            $wanted = ($keepFiles -contains $name) -or
                ($keepDirs | Where-Object { $name.StartsWith($_) })
            if (-not $wanted -or $name.EndsWith('/')) { continue }

            $relative = $name.Substring('pgsql/'.Length)
            $target = [IO.Path]::GetFullPath((Join-Path $Destination $relative))
            if (-not $target.StartsWith($destRoot, [StringComparison]::OrdinalIgnoreCase)) {
                throw "Refusing archive entry outside the destination: $name"
            }
            New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $target, $true)
            $count++
        }
    } finally {
        $archive.Dispose()
    }

    $postgres = Join-Path $Destination 'bin\postgres.exe'
    if (-not (Test-Path -LiteralPath $postgres)) { throw "bin\postgres.exe missing after extraction" }
    Write-Host "Extracted $count files to $Destination"

    if (-not $VcRedistDir) { $VcRedistDir = Find-VcRedistDir }
    $bin = Join-Path $Destination 'bin'
    foreach ($dll in $vcRuntimeDlls) {
        $source = Join-Path $VcRedistDir $dll
        if (-not (Test-Path -LiteralPath $source)) { throw "VC++ runtime $dll missing from $VcRedistDir" }
        Copy-Item -LiteralPath $source -Destination $bin -Force
    }
    Write-Host "VC++ runtime copied from $VcRedistDir"
    & (Join-Path $PSScriptRoot 'check-pe-imports.ps1') -Path $bin, (Join-Path $Destination 'lib') -ResolveDir $bin
    if ($LASTEXITCODE) { throw "a bundled Postgres binary does not find its VC++ runtime (see above)" }
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
