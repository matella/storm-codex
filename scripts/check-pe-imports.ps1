<#
.SYNOPSIS
    Checks that the Windows binaries shipped by the desktop app find their Visual C++ runtime.

.DESCRIPTION
    Reads the import table (and the delay-load import table) of every .exe and .dll under -Path, without loading
    them, and fails when one imports a Visual C++ runtime DLL (vcruntime*, msvcp*, concrt*, vccorlib*) that is not
    next to it or in one of -ResolveDir. A clean Windows install without the VC++ redistributable has none of them,
    so every such import must be satisfied inside the bundle (app-local deployment). The Universal CRT
    (ucrtbase.dll, api-ms-win-crt-*) is part of Windows 10 and later and is not checked.

.PARAMETER Path
    Files or folders to check (folders: every .exe and .dll in them, not recursive).

.PARAMETER ResolveDir
    Extra folders where an import may be found (e.g. postgres/bin for the extension modules in postgres/lib, which
    postgres.exe loads: their dependencies are searched in the application's folder first).

.PARAMETER List
    Print the VC++ runtime imports of every file, not only the failures.

.EXAMPLE
    pwsh scripts/check-pe-imports.ps1 -Path target/postgres/bin, target/postgres/lib -ResolveDir target/postgres/bin
    pwsh scripts/check-pe-imports.ps1 -Path target/release/storm-codex-server.exe
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string[]]$Path,
    [string[]]$ResolveDir = @(),
    [switch]$List
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$vcRuntime = '^(vcruntime|msvcp|concrt|vccorlib)\d'

function Get-PeImports([string]$File) {
    $b = [IO.File]::ReadAllBytes($File)
    if ($b.Length -lt 0x40 -or $b[0] -ne 0x4D -or $b[1] -ne 0x5A) { return $null }
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    if ($pe -lt 0 -or $pe + 24 -gt $b.Length -or [BitConverter]::ToUInt32($b, $pe) -ne 0x4550) { return $null }
    $coff = $pe + 4
    $sections = [BitConverter]::ToUInt16($b, $coff + 2)
    $optSize = [BitConverter]::ToUInt16($b, $coff + 16)
    $opt = $coff + 20
    $magic = [BitConverter]::ToUInt16($b, $opt)
    $dirs = switch ($magic) { 0x20B { $opt + 112 } 0x10B { $opt + 96 } default { return $null } }
    $secTable = $opt + $optSize
    $toOffset = {
        param([uint32]$rva)
        for ($i = 0; $i -lt $sections; $i++) {
            $s = $secTable + 40 * $i
            $va = [BitConverter]::ToUInt32($b, $s + 12)
            $size = [Math]::Max([BitConverter]::ToUInt32($b, $s + 8), [BitConverter]::ToUInt32($b, $s + 16))
            if ($rva -ge $va -and $rva -lt $va + $size) {
                return [int64]$rva - $va + [BitConverter]::ToUInt32($b, $s + 20)
            }
        }
        return -1
    }
    $readName = {
        param([uint32]$rva)
        $o = & $toOffset $rva
        if ($o -lt 0) { return $null }
        $end = $o
        while ($end -lt $b.Length -and $b[$end] -ne 0) { $end++ }
        return [Text.Encoding]::ASCII.GetString($b, $o, $end - $o)
    }
    $names = New-Object System.Collections.Generic.List[string]
    # data directory 1 = imports (20-byte descriptors, name RVA at +12); 13 = delay-load imports (32 bytes, +4)
    foreach ($d in @(@{ Index = 1; Size = 20; NameAt = 12 }, @{ Index = 13; Size = 32; NameAt = 4 })) {
        $rva = [BitConverter]::ToUInt32($b, $dirs + 8 * $d.Index)
        if ($rva -eq 0) { continue }
        $o = & $toOffset $rva
        if ($o -lt 0) { continue }
        while ($o + $d.Size -le $b.Length) {
            $nameRva = [BitConverter]::ToUInt32($b, $o + $d.NameAt)
            if ($nameRva -eq 0) { break }
            $n = & $readName $nameRva
            if ($n) { $names.Add($n) }
            $o += $d.Size
        }
    }
    return , $names.ToArray()
}

$files = foreach ($p in $Path) {
    if (Test-Path -LiteralPath $p -PathType Container) {
        Get-ChildItem -LiteralPath $p -File | Where-Object { $_.Extension -in '.exe', '.dll' }
    } elseif (Test-Path -LiteralPath $p -PathType Leaf) {
        Get-Item -LiteralPath $p
    } else {
        throw "not found: $p"
    }
}

$failures = 0
$checked = 0
foreach ($f in $files) {
    $imports = Get-PeImports $f.FullName
    if ($null -eq $imports) { continue }
    $checked++
    $vc = @($imports | Where-Object { $_ -match $vcRuntime } | Sort-Object -Unique)
    if ($List -and $vc.Count -gt 0) { Write-Host ("{0}: {1}" -f $f.Name, ($vc -join ', ')) }
    foreach ($dll in $vc) {
        $found = @($f.DirectoryName) + $ResolveDir | Where-Object { Test-Path -LiteralPath (Join-Path $_ $dll) }
        if (-not $found) {
            Write-Host "MISSING: $($f.FullName) imports $dll, not found next to it or in -ResolveDir" -ForegroundColor Red
            $failures++
        }
    }
}
Write-Host "$checked PE file(s) checked, $failures unresolved VC++ runtime import(s)"
if ($checked -eq 0) { throw "no PE file found under: $($Path -join ', ')" }
if ($failures -gt 0) { exit 1 }
exit 0
