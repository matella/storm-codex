# Jalon 0 — échantillon stratifié de 50 replays (2023→2026) vers corpus/spike50
$src = "$env:USERPROFILE\Documents\Heroes of the Storm\Accounts\*\*\Replays\Multiplayer"
$dst = Join-Path $PSScriptRoot "..\corpus\spike50"
New-Item -ItemType Directory -Force $dst | Out-Null
Get-ChildItem $dst -Filter *.StormReplay -ErrorAction SilentlyContinue | Remove-Item

$all = Get-ChildItem "$src\*.StormReplay" | Sort-Object LastWriteTime
$pick = [System.Collections.Generic.List[object]]::new()
$all | Select-Object -Last 3 | ForEach-Object { $pick.Add($_) }            # 3 plus récents garantis
$years = $all | Group-Object { $_.LastWriteTime.Year } | Sort-Object Name
$quota = [Math]::Ceiling(50 / [Math]::Max(1, $years.Count))
foreach ($g in $years) {
  $year = @($g.Group | Where-Object { $_ -notin $pick })
  if ($year.Count -gt 0) {
    $year | Get-Random -Count ([Math]::Min($quota, $year.Count)) | ForEach-Object { $pick.Add($_) }
  }
}
$rest = $all | Where-Object { $_ -notin $pick } | Sort-Object LastWriteTime -Descending
$rest | Select-Object -First ([Math]::Max(0, 50 - $pick.Count)) | ForEach-Object { $pick.Add($_) }
$pick = $pick | Select-Object -First 50
$pick | ForEach-Object { Copy-Item $_.FullName $dst }
$pick | Sort-Object LastWriteTime |
  Select-Object @{n='name';e={$_.Name}}, @{n='bytes';e={$_.Length}},
                @{n='mtime';e={$_.LastWriteTime.ToString('s')}} |
  Export-Csv (Join-Path $dst 'manifest.csv') -NoTypeInformation
"$((Get-ChildItem $dst -Filter *.StormReplay).Count) replays copiés"
($pick | Group-Object { $_.LastWriteTime.Year } | Sort-Object Name | ForEach-Object { "$($_.Name): $($_.Count)" }) -join ' · '
