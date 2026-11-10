# Serve a built spike (a `dist` folder) with the real server, replacing any server already on that port.
# Usage: ./serve.ps1 -Port 8095 -Dist leptos-profile/dist
# The server reads index.html once at startup, so run this again after each rebuild.
# Needs the dev Postgres (docker-compose.dev.yml) and a debug build of storm-codex-server.
param([int]$Port, [string]$Dist)
$root = Resolve-Path "$PSScriptRoot/../.."
$old = Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
if ($old) {
    # only ever replace our own server: another program may own the port
    $proc = Get-Process -Id $old.OwningProcess -ErrorAction SilentlyContinue
    if ($proc.ProcessName -ne 'storm-codex-server') { throw "Port $Port is used by '$($proc.ProcessName)', pick another port." }
    Stop-Process -Id $old.OwningProcess -Force; Start-Sleep 2
}
$env:BIND_ADDR = "127.0.0.1:$Port"
$env:WEB_DIR = (Resolve-Path (Join-Path $PSScriptRoot $Dist)).Path
$env:DATABASE_URL = 'postgres://storm:storm@localhost:5433/storm_codex'
$env:HOTSPATCHNOTES_URL = 'http://127.0.0.1:1'
Start-Process -FilePath "$root/target/debug/storm-codex-server.exe" -WindowStyle Hidden
Start-Sleep 6
(Invoke-WebRequest "http://127.0.0.1:$Port/" -UseBasicParsing).StatusCode
