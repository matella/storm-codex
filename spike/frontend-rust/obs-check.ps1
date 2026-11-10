# Prepares the real-OBS check of the Leptos overlay (step 0 go/no-go, operator's check).
# 1. builds the overlay (trunk, release), 2. serves it with the real server on port 8097,
# 3. prints the URL to paste in an OBS "Browser" source (1920x1080) and the checklist.
# Needs the dev Postgres (docker-compose.dev.yml) and a debug build of storm-codex-server.
param([int]$Port = 8097)
Push-Location "$PSScriptRoot/leptos-overlay"
$env:NO_COLOR = 'true'   # trunk rejects NO_COLOR=1, which some shells set
trunk build --release
if ($LASTEXITCODE -ne 0) { Pop-Location; throw 'trunk build failed' }
Pop-Location
& "$PSScriptRoot/serve.ps1" -Port $Port -Dist leptos-overlay/dist
@"

OBS: Sources > + > Browser, URL  http://127.0.0.1:$Port/cast/overlay  (width 1920, height 1080,
'Shutdown source when not visible' off). Open the cast control room of the REAL app to drive the scenes,
or POST /api/cast/scene on this server ({"scene":"lineups|series|draft|ingame|hide"}).

Checklist (write what you see in spike/frontend-rust/RESULTS.md):
 [ ] the page loads (WebAssembly runs in OBS's browser) and shows the first scene; note how long it takes
     from adding the source / refreshing it (Refresh cache of current page) until the scene is visible
 [ ] transparent background (the video below shows through, no black/white box)
 [ ] fonts (Cinzel / Oswald) render, hero portraits and map images show
 [ ] scene change keeps the 300 ms cross-fade: lineups, series (with a game that has a replay summary
     for the post-game slides), draft, ingame, hide
 [ ] a change made in the control room appears within a second (WebSocket); stop and restart the server
     and check the overlay reconnects by itself
 [ ] CPU use of OBS with the source visible stays normal (Stats window), no dropped frames
"@
