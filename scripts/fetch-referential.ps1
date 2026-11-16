<#
.SYNOPSIS
    Downloads the pinned reference-data snapshot (referential.tar.gz) for the desktop installer.

.DESCRIPTION
    Reads the URL and SHA-256 from desktop/referential-pin.txt, refuses a placeholder pin, downloads
    the file (or uses -FilePath), refuses it unless the SHA-256 matches, and writes it to
    -Destination. The snapshot itself is built by scripts/make-referential.sh.

.PARAMETER Destination
    Output file (default: target/referential/referential.tar.gz, the path desktop/tauri.bundle.conf.json bundles).

.PARAMETER FilePath
    Use a local snapshot instead of downloading. It is still hash-checked against the pin.

.EXAMPLE
    pwsh scripts/fetch-referential.ps1
#>
[CmdletBinding()]
param(
    [string]$Destination,
    [string]$FilePath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
if (-not $Destination) { $Destination = Join-Path $repoRoot 'target\referential\referential.tar.gz' }
$pinFile = Join-Path $repoRoot 'desktop\referential-pin.txt'

$pin = @{}
foreach ($line in Get-Content -LiteralPath $pinFile) {
    $line = $line.Trim()
    if ($line -eq '' -or $line.StartsWith('#')) { continue }
    $key, $value = $line -split '=', 2
    $pin[$key.Trim()] = $value.Trim()
}
foreach ($key in 'url', 'sha256') {
    if (-not $pin.ContainsKey($key) -or -not $pin[$key]) { throw "referential-pin.txt: missing '$key'" }
}
if ($pin.url.StartsWith('PLACEHOLDER') -or $pin.sha256.StartsWith('PLACEHOLDER')) {
    throw ("referential-pin.txt is still a placeholder: build the snapshot with scripts/make-referential.sh, " +
        "upload it as a release asset and pin its URL and SHA-256 (docs/spec/09-operations.md, Releasing the desktop app)")
}
if ($pin.sha256 -notmatch '^[0-9a-fA-F]{64}$') { throw "referential-pin.txt: sha256 is not 64 hex characters" }
if (-not $pin.url.StartsWith('https://')) { throw "referential-pin.txt: url must be https" }

$work = Join-Path ([IO.Path]::GetTempPath()) ("storm-codex-ref-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    if ($FilePath) {
        $file = (Resolve-Path -LiteralPath $FilePath).Path
    } else {
        $file = Join-Path $work 'referential.tar.gz'
        Write-Host "Downloading $($pin.url)"
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -Uri $pin.url -OutFile $file -UseBasicParsing
    }

    $actual = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $pin.sha256.ToLowerInvariant()) {
        throw "SHA-256 mismatch for $file`n  expected $($pin.sha256)`n  actual   $actual"
    }
    Write-Host "SHA-256 verified: $actual"

    New-Item -ItemType Directory -Path (Split-Path -Parent $Destination) -Force | Out-Null
    Copy-Item -LiteralPath $file -Destination $Destination -Force
    Write-Host "Snapshot written to $Destination ($((Get-Item -LiteralPath $Destination).Length) bytes)"
} finally {
    Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
}
