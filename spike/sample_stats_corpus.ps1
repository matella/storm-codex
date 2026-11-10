# Milestone 2 — stats parity corpus: >= 3 replays per map (map name = end of the client-rs
# file name "YYYY-MM-DD HH.mm.ss <Map>.StormReplay") + the whole spike50 corpus.
$src = "$env:USERPROFILE\Documents\Heroes of the Storm\Accounts\*\*\Replays\Multiplayer"
$dst = Join-Path $PSScriptRoot "..\corpus\stats"
New-Item -ItemType Directory -Force $dst | Out-Null

$all = Get-ChildItem "$src\*.StormReplay"
$byMap = $all | Group-Object { ($_.BaseName -replace '^\d{4}-\d{2}-\d{2} \d{2}\.\d{2}\.\d{2} ', '') }
$pick = [System.Collections.Generic.List[object]]::new()
foreach ($g in $byMap | Sort-Object Name) {
  # most recent first (builds covered by hots-parser via override), then an old one
  $sorted = $g.Group | Sort-Object LastWriteTime -Descending
  $sel = @($sorted | Select-Object -First 2) + @($sorted | Select-Object -Last 1) | Select-Object -Unique
  $sel | ForEach-Object { $pick.Add($_) }
  "{0,-30} {1} replays (pris {2})" -f $g.Name, $g.Count, ($sel.Count)
}
$pick | ForEach-Object { Copy-Item $_.FullName $dst -Force }
Get-ChildItem (Join-Path $PSScriptRoot "..\corpus\spike50\*.StormReplay") | Copy-Item -Destination $dst -Force
"total corpus/stats: $((Get-ChildItem $dst -Filter *.StormReplay).Count) replays"