# Jalon 2 — corpus de parité stats : >= 3 replays par carte (nom de carte = fin du nom de
# fichier client-rs "YYYY-MM-DD HH.mm.ss <Carte>.StormReplay") + tout le corpus spike50.
$src = "$env:USERPROFILE\Documents\Heroes of the Storm\Accounts\*\*\Replays\Multiplayer"
$dst = Join-Path $PSScriptRoot "..\corpus\stats"
New-Item -ItemType Directory -Force $dst | Out-Null

$all = Get-ChildItem "$src\*.StormReplay"
$byMap = $all | Group-Object { ($_.BaseName -replace '^\d{4}-\d{2}-\d{2} \d{2}\.\d{2}\.\d{2} ', '') }
$pick = [System.Collections.Generic.List[object]]::new()
foreach ($g in $byMap | Sort-Object Name) {
  # les plus récents d'abord (builds couverts par hots-parser via override), puis un ancien
  $sorted = $g.Group | Sort-Object LastWriteTime -Descending
  $sel = @($sorted | Select-Object -First 2) + @($sorted | Select-Object -Last 1) | Select-Object -Unique
  $sel | ForEach-Object { $pick.Add($_) }
  "{0,-30} {1} replays (pris {2})" -f $g.Name, $g.Count, ($sel.Count)
}
$pick | ForEach-Object { Copy-Item $_.FullName $dst -Force }
Get-ChildItem (Join-Path $PSScriptRoot "..\corpus\spike50\*.StormReplay") | Copy-Item -Destination $dst -Force
"total corpus/stats : $((Get-ChildItem $dst -Filter *.StormReplay).Count) replays"